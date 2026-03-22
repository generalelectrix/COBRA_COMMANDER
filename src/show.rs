use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use std::env::current_exe;

use crate::{
    animation::AnimationUIState,
    animation_visualizer::{AnimationPublisher, AnimationServiceState, animation_publisher},
    channel::{ChannelStateEmitter, Channels, STROBE_CONTROL_CHANNEL},
    cli::Command,
    clocks::Clocks,
    color::Hsluv,
    control::{ControlMessage, Controller, MetaCommand, meta_command_from_osc},
    dmx::DmxBuffer,
    fixture::{
        Patch, animation_target::ControllableTargetedAnimation, prelude::FixtureGroupUpdate,
    },
    master::MasterControls,
    midi::{EmitMidiChannelMessage, MidiControlMessage, MidiHandler},
    osc::{OscControlMessage, ScopedControlEmitter},
    preview::Previewer,
};

pub use crate::channel::ChannelId;
use tunnels::audio::AudioInput;

use anyhow::{Context as _, Result, bail};
use color_organ::{HsluvColor, IgnoreEmitter};
use log::error;
use rust_dmx::{DmxPort, OfflineDmxPort};

pub struct Show {
    zmq_ctx: zmq::Context,
    controller: Controller,
    dmx_ports: Vec<Box<dyn DmxPort>>,
    dmx_buffers: Vec<DmxBuffer>,
    patch: Patch,
    patch_file_path: PathBuf,
    channels: Channels,
    master_controls: MasterControls,
    animation_ui_state: AnimationUIState,
    clocks: Clocks,
    animation_service: Option<AnimationPublisher>,
    preview: Previewer,
    master_strobe_channel: bool,
}

const CONTROL_TIMEOUT: Duration = Duration::from_micros(500);
/// The enttec hypothetically outputs 40 fps. This seems to only truly be the
/// case when no writes are being performed. Writing at the port framerate (or
/// even twice as fast) seems to bring the framerate down a bit - adding about
/// 300 us or so in between frames. Thus this slightly odd update interval.
pub const UPDATE_INTERVAL: Duration = Duration::from_micros(25300);

impl Show {
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        patch: Patch,
        patch_file_path: PathBuf,
        controller: Controller,
        dmx_ports: Vec<Box<dyn DmxPort>>,
        clocks: Clocks,
        animation_service: Option<AnimationPublisher>,
        preview: Previewer,
        master_strobe_channel: bool,
        zmq_ctx: zmq::Context,
    ) -> Result<Self> {
        let channels = Channels::from_iter(patch.channels().cloned());

        if master_strobe_channel && channels.validate_channel(STROBE_CONTROL_CHANNEL).is_ok() {
            bail!(
                "cannot use a master strobe channel since the channel is already in use for a fixture group"
            );
        }

        let initial_channel = channels.current_channel();
        let animation_ui_state = AnimationUIState::new(initial_channel);

        let mut show = Self {
            zmq_ctx,
            controller,
            dmx_buffers: vec![[0u8; 512]; dmx_ports.len()],
            dmx_ports,
            patch,
            patch_file_path,
            channels,
            master_controls: Default::default(),
            animation_ui_state,
            clocks,
            animation_service,
            preview,
            master_strobe_channel,
        };
        show.reconcile_submaster_wings()?;
        show.reconcile_clock_wing()?;
        show.refresh_ui();
        Ok(show)
    }

    /// Run the show forever in the current thread.
    pub fn run(&mut self) {
        let mut last_update = Instant::now();

        loop {
            // Process a control event if one is pending.
            if let Err(err) = self.control(CONTROL_TIMEOUT) {
                error!("A control error occurred: {err:#}.");
            }

            // Compute updates until we're current.
            let mut now = Instant::now();
            let mut time_since_last_update = now - last_update;
            let mut should_render = false;
            while time_since_last_update > UPDATE_INTERVAL {
                // Update the state of the show.
                self.update(UPDATE_INTERVAL);
                should_render = true;

                last_update += UPDATE_INTERVAL;
                now = Instant::now();
                time_since_last_update = now - last_update;
            }

            // Render the state of the show.
            if should_render {
                self.render();
                for (port, buffer) in self.dmx_ports.iter_mut().zip(&self.dmx_buffers) {
                    if let Err(e) = port.write(buffer) {
                        error!("DMX write error: {e:#}.");
                    }
                }
            }
        }
    }

    /// Handle at most one control message.
    ///
    /// Wait for the provided duration for a message to appear.
    fn control(&mut self, timeout: Duration) -> Result<()> {
        let msg = match self.controller.recv(timeout)? {
            Some(m) => m,
            None => {
                return Ok(());
            }
        };

        match msg {
            ControlMessage::RegisterClient(client_id) => {
                println!("Registering new OSC client at {client_id}.");
                self.controller.register_osc_client(client_id);
                self.refresh_ui();
                Ok(())
            }
            ControlMessage::DeregisterClient(client_id) => {
                println!("Deregistering OSC client at {}.", client_id);
                self.controller.deregister_osc_client(client_id);
                Ok(())
            }
            ControlMessage::MidiDeviceChange(change) => {
                let needs_ui_refresh = self.controller.handle_device_change(change)?;
                if needs_ui_refresh {
                    self.refresh_ui();
                }
                Ok(())
            }
            ControlMessage::Midi(msg) => self.handle_midi_message(&msg),
            ControlMessage::Osc(msg) => self.handle_osc_message(&msg),
            ControlMessage::Meta(cmd, reply) => {
                let result = self.handle_meta_command(cmd);
                if let Some(reply) = reply {
                    let _ = reply.send(result.as_ref().map_err(|e| format!("{e:#}")).copied());
                }
                result
            }
        }
    }

    /// Handle a meta-command.
    fn handle_meta_command(&mut self, cmd: MetaCommand) -> Result<()> {
        match cmd {
            MetaCommand::ReloadPatch => {
                self.patch.repatch_from_file(&self.patch_file_path)?;
                let sender = self.controller.sender_with_metadata(None);
                sender.emit_midi_channel_message(&crate::channel::StateChange::Clear);
                Channels::emit_osc_state_change(
                    crate::channel::StateChange::Clear,
                    &ScopedControlEmitter {
                        entity: crate::osc::channels::GROUP,
                        emitter: &sender,
                    },
                );
                self.channels = Channels::from_iter(self.patch.channels().cloned());
                self.reconcile_submaster_wings()?;
                self.refresh_ui();
                let new_universe_count = self.patch.universe_count();
                let current_len = self.dmx_ports.len();
                if new_universe_count > current_len {
                    for _ in current_len..new_universe_count {
                        self.dmx_ports
                            .push(Box::new(OfflineDmxPort) as Box<dyn DmxPort>);
                        self.dmx_buffers.push([0u8; 512]);
                    }
                } else if new_universe_count < current_len {
                    self.dmx_ports.truncate(new_universe_count);
                    self.dmx_buffers.truncate(new_universe_count);
                }
                for buf in &mut self.dmx_buffers {
                    buf.fill(0);
                }
                Ok(())
            }
            MetaCommand::RefreshUI => {
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::ResetAllAnimations => {
                for group in self.patch.iter_mut() {
                    group.reset_animations();
                }
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::AssignDmxPort { universe, port } => {
                assign_dmx_port(&mut self.dmx_ports, &mut self.dmx_buffers, universe, port)
            }
            MetaCommand::AddMidiDevice(spec) => {
                self.controller.add_midi_device(spec)?;
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::ClearMidiDevice { slot_name } => {
                self.controller.clear_midi_device(&slot_name)?;
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::UseClockService(service) => {
                self.clocks = Clocks::Service(service);
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::UseInternalClocks(device_name) => {
                let audio_input = device_name
                    .map(|name| AudioInput::new(Some(name)))
                    .transpose()?;
                self.clocks = Clocks::internal(audio_input);
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(())
            }
            MetaCommand::StartAnimationVisualizer => {
                if self.animation_service.is_none() {
                    self.animation_service = Some(animation_publisher(&self.zmq_ctx)?);
                }
                let bin_path =
                    current_exe().context("failed to get the path to the running binary")?;
                std::process::Command::new(bin_path)
                    .arg(Command::Viz.to_string())
                    .spawn()
                    .context("failed to start animation visualizer")?;
                Ok(())
            }
        }
    }

    /// Handle a single MIDI control message.
    fn handle_midi_message(&mut self, msg: &MidiControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(None);
        let Some(show_ctrl_msg) = msg.device.interpret(&msg.event) else {
            return Ok(());
        };
        match show_ctrl_msg {
            ShowControlMessage::Channel(msg) => {
                if self.master_strobe_channel
                    && let crate::channel::ControlMessage::Control { channel_id, msg } = &msg
                    && *channel_id == Some(STROBE_CONTROL_CHANNEL)
                {
                    self.master_controls.handle_strobe_channel(msg, &sender);
                } else {
                    self.channels.control(
                        &msg,
                        &mut self.patch,
                        &self.animation_ui_state,
                        &sender,
                    )?;
                }
            }
            ShowControlMessage::Clock(msg) => self.clocks.control_clock(msg, sender.controller),
            ShowControlMessage::Audio(msg) => self.clocks.control_audio(msg, &mut self.controller),
            ShowControlMessage::Master(msg) => {
                self.master_controls.control(&msg, &sender);
            }
            ShowControlMessage::Animation(msg) => {
                let Some(channel) = self.channels.current_channel() else {
                    bail!(
                        "cannot handle animation control message because no channel is selected\n{msg:?}"
                    );
                };
                self.animation_ui_state.control(
                    msg,
                    channel,
                    self.channels
                        .group_by_channel_mut(&mut self.patch, channel)?,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter: &sender,
                    },
                )?;
            }
            ShowControlMessage::ColorOrgan(msg) => {
                // FIXME: this is really janky and has no way to route messages.
                for group in self.patch.iter_mut() {
                    let Some(color_organ) = group.color_organ_mut() else {
                        continue;
                    };
                    color_organ.control(msg.clone(), &IgnoreEmitter);
                }
            }
        };
        Ok(())
    }

    /// Handle a single OSC message.
    fn handle_osc_message(&mut self, msg: &OscControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(Some(&msg.client_id));

        match msg.group() {
            "Meta" => {
                if let Some(cmd) = meta_command_from_osc(msg)? {
                    self.handle_meta_command(cmd)
                } else {
                    Ok(())
                }
            }
            crate::master::GROUP => self.master_controls.control_osc(msg, &sender),
            crate::osc::channels::GROUP => {
                self.channels
                    .control_osc(msg, &mut self.patch, &self.animation_ui_state, &sender)
            }
            crate::osc::animation::GROUP => {
                let Some(channel) = self.channels.current_channel() else {
                    bail!(
                        "cannot handle animation control message because no channel is selected\n{msg:?}"
                    );
                };
                self.animation_ui_state.control_osc(
                    msg,
                    channel,
                    self.channels
                        .group_by_channel_mut(&mut self.patch, channel)?,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter: &sender,
                    },
                )
            }
            crate::osc::audio::GROUP => self.clocks.control_audio_osc(msg, &mut self.controller),
            crate::osc::clock::GROUP => self.clocks.control_clock_osc(msg, &mut self.controller),
            // Assume any other control group is referring to a fixture group.
            fixture_group => self.patch.get_mut(fixture_group)?.control(
                msg,
                ChannelStateEmitter::new(self.channels.channel_for_fixture(fixture_group), &sender),
            ),
        }
    }

    /// Update the state of the show using the provided timestep.
    fn update(&mut self, delta_t: Duration) {
        self.clocks.update(delta_t, &mut self.controller);

        let clock_state = self.clocks.get();
        self.master_controls.clock_state = clock_state.clock_bank;
        self.master_controls.audio_envelope = clock_state.audio_envelope;

        self.master_controls
            .update(delta_t, &self.controller.sender_with_metadata(None));

        let mut flash_distributor = self
            .master_controls
            .flash_distributor(self.patch.iter().filter(|g| g.strobe_enabled()).count());

        for group in self.patch.iter_mut() {
            group.update(
                FixtureGroupUpdate {
                    master_controls: &self.master_controls,
                    flash_now: flash_distributor.flash_now(group.strobe_enabled()),
                },
                delta_t,
            );
        }

        if let Err(err) = self.publish_animation_state() {
            error!("Animation state publishing error: {err}.");
        };
    }

    /// Render the state of the show out to DMX.
    fn render(&mut self) {
        self.preview.start_frame();
        // NOTE: we don't bother to empty the buffer because we will always
        // overwrite all previously-rendered state.
        for group in self.patch.iter() {
            group.render(&self.master_controls, &mut self.dmx_buffers, &self.preview);
        }
    }

    /// Reconcile MIDI submaster wing slots with the current channel count.
    fn reconcile_submaster_wings(&mut self) -> Result<()> {
        self.controller
            .reconcile_submaster_wings(self.channels.channel_count())
    }

    /// Reconcile the clock wing slot with the current clock mode.
    fn reconcile_clock_wing(&mut self) -> Result<()> {
        self.controller
            .reconcile_clock_wing(self.clocks.is_internal())
    }

    /// Send messages to refresh all UI state.
    fn refresh_ui(&mut self) {
        let emitter = &self.controller.sender_with_metadata(None);
        for (key, group) in self.patch.iter_with_keys() {
            group.emit_state(ChannelStateEmitter::new(
                self.channels.channel_for_fixture(key),
                emitter,
            ));
        }

        self.master_controls.emit_state(emitter);

        self.channels.emit_state(false, &self.patch, emitter);

        if let Some(current_channel) = self.channels.current_channel() {
            if let Ok(group) = self.channels.group_by_channel(&self.patch, current_channel) {
                self.animation_ui_state.emit_state(
                    current_channel,
                    group,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter,
                    },
                );
            } else {
                error!(
                    "Refreshing UI: could not get fixture group for current channel {current_channel}."
                );
            }
        }

        self.clocks.emit_state(&mut self.controller);
    }

    /// If we have a animation publisher configured, send current state.
    fn publish_animation_state(&mut self) -> Result<()> {
        let Some(anim_pub) = self.animation_service.as_mut() else {
            return Ok(());
        };
        let Some(current_channel) = self.channels.current_channel() else {
            return Ok(());
        };
        let group = self
            .channels
            .group_by_channel(&self.patch, current_channel)?;
        let animation_index = self
            .animation_ui_state
            .animation_index_for_channel(current_channel);
        // FIXME: would be nice to avoid the extra memcopy here...
        anim_pub.send(&AnimationServiceState {
            animation: group
                .get_animation(animation_index)
                .map(ControllableTargetedAnimation::anim)
                .cloned()
                .unwrap_or_default(),
            clocks: self.clocks.get(),
            fixture_count: group.fixture_configs().len(),
        })
    }
}

/// Assign a DMX port to a universe.
///
/// Validates the universe index, opens the port, zeros the DMX buffer,
/// and swaps the port into place.
fn assign_dmx_port(
    dmx_ports: &mut [Box<dyn DmxPort>],
    dmx_buffers: &mut [DmxBuffer],
    universe: usize,
    mut port: Box<dyn DmxPort>,
) -> Result<()> {
    if universe >= dmx_ports.len() {
        bail!(
            "universe {universe} out of range (show has {} universe(s))",
            dmx_ports.len()
        );
    }
    port.open()
        .map_err(|e| anyhow::anyhow!("failed to open port {port}: {e}"))?;
    dmx_buffers[universe].fill(0);
    dmx_ports[universe] = port;
    Ok(())
}

/// Strongly-typed top-level show control messages.
/// These cover all of the fixed control features, but not fixture-specific controls.
#[derive(Debug, Clone)]
pub enum ShowControlMessage {
    // Unused because show control messages only come from OSC so far.
    #[allow(unused)]
    Master(crate::master::ControlMessage),
    Channel(crate::channel::ControlMessage),
    Clock(tunnels::clock_bank::ControlMessage),
    // Unused because show control messages only come from OSC so far.
    #[allow(unused)]
    Animation(crate::animation::ControlMessage),
    Audio(tunnels::audio::ControlMessage),
    ColorOrgan(color_organ::ControlMessage<Hsluv>),
}

impl From<Hsluv> for HsluvColor {
    fn from(c: Hsluv) -> Self {
        Self::new(c.hue, c.sat, c.lightness)
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::control::CommandClient;

    /// Stand up a fully-contained test Show processing commands on a
    /// background thread.  Returns a real `CommandClient` connected to it.
    ///
    /// The Show lives until the returned `CommandClient` (and any clones)
    /// are dropped, at which point the background thread exits.
    pub fn test_show_client() -> CommandClient {
        let (send_tx, send_rx) = std::sync::mpsc::sync_channel(0);

        std::thread::spawn(move || {
            let dir = tempfile::tempdir().unwrap();
            let patch_path = dir.path().join("patch.yaml");
            std::fs::write(&patch_path, "- fixture: Dimmer\n  patches:\n    - addr: 1\n").unwrap();
            let patch = Patch::from_file(&patch_path).unwrap();
            let (mut show, send) = Show::test_new(patch, patch_path);
            let zmq_ctx = show.zmq_ctx.clone();

            // Send the client handle back to the calling thread.
            send_tx.send(CommandClient::new(send, zmq_ctx)).unwrap();

            // Process commands until the client is dropped.
            loop {
                match show.control(Duration::from_millis(100)) {
                    Ok(()) => {}
                    Err(e) if e.to_string().contains("disconnected") => break,
                    Err(_) => {} // command errors are expected, keep running
                }
            }
        });

        send_rx.recv().unwrap()
    }
}

#[cfg(test)]
impl Show {
    fn test_new(
        patch: Patch,
        patch_file_path: PathBuf,
    ) -> (Self, std::sync::mpsc::Sender<ControlMessage>) {
        let (controller, send) = Controller::test_new();
        let universe_count = patch.universe_count();
        let channels = Channels::from_iter(patch.channels().cloned());
        let initial_channel = channels.current_channel();
        let mut show = Self {
            zmq_ctx: zmq::Context::new(),
            controller,
            dmx_buffers: vec![[0u8; 512]; universe_count],
            dmx_ports: (0..universe_count)
                .map(|_| Box::new(OfflineDmxPort) as Box<dyn DmxPort>)
                .collect(),
            patch,
            patch_file_path,
            channels,
            master_controls: Default::default(),
            animation_ui_state: AnimationUIState::new(initial_channel),
            clocks: Clocks::test_new(),
            animation_service: None,
            preview: Previewer::Off,
            master_strobe_channel: false,
        };
        show.reconcile_submaster_wings().unwrap();
        show.reconcile_clock_wing().unwrap();
        (show, send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dmx::mock::MockDmxPort;

    const ONE_UNIVERSE_PATCH: &str = "\
- fixture: Dimmer
  patches:
    - addr: 1
";
    const TWO_UNIVERSE_PATCH: &str = "\
- fixture: Dimmer
  patches:
    - addr: 1
    - addr: 1
      universe: 1
";

    /// Create a Show backed by a temporary patch file.
    /// Returns the TempDir too — it must outlive the Show for reload tests.
    fn show_from_yaml(yaml: &str) -> (Show, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let patch_path = dir.path().join("patch.yaml");
        std::fs::write(&patch_path, yaml).unwrap();
        let patch = Patch::from_file(&patch_path).unwrap();
        let (show, _send) = Show::test_new(patch, patch_path);
        (show, dir)
    }

    #[test]
    fn assign_dmx_port_success() {
        let (mut show, _dir) = show_from_yaml(TWO_UNIVERSE_PATCH);
        show.dmx_buffers[1].fill(0xFF);

        show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 1,
            port: Box::new(MockDmxPort::new()),
        })
        .unwrap();

        assert!(show.dmx_buffers[1].iter().all(|&b| b == 0));
        assert_eq!(format!("{}", show.dmx_ports[1]), "mock");
    }

    #[test]
    fn assign_dmx_port_open_fails() {
        let (mut show, _dir) = show_from_yaml(ONE_UNIVERSE_PATCH);

        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 0,
            port: Box::new(MockDmxPort::failing()),
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed to open port"));
    }

    #[test]
    fn assign_dmx_port_universe_out_of_range() {
        let (mut show, _dir) = show_from_yaml(TWO_UNIVERSE_PATCH);

        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 5,
            port: Box::new(MockDmxPort::new()),
        });
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("out of range"));
        assert!(err_msg.contains("2 universe(s)"));
    }

    #[test]
    fn reload_patch_grows_universes() {
        let (mut show, dir) = show_from_yaml(ONE_UNIVERSE_PATCH);
        assert_eq!(show.dmx_ports.len(), 1);

        std::fs::write(dir.path().join("patch.yaml"), TWO_UNIVERSE_PATCH).unwrap();
        show.handle_meta_command(MetaCommand::ReloadPatch).unwrap();
        assert_eq!(show.dmx_ports.len(), 2);
        assert_eq!(show.dmx_buffers.len(), 2);
    }

    #[test]
    fn reload_patch_shrinks_universes() {
        let (mut show, dir) = show_from_yaml(TWO_UNIVERSE_PATCH);
        assert_eq!(show.dmx_ports.len(), 2);

        std::fs::write(dir.path().join("patch.yaml"), ONE_UNIVERSE_PATCH).unwrap();
        show.handle_meta_command(MetaCommand::ReloadPatch).unwrap();
        assert_eq!(show.dmx_ports.len(), 1);
        assert_eq!(show.dmx_buffers.len(), 1);
    }

}
