use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    animation::AnimationUIState,
    channel::{ChannelStateEmitter, Channels, strobe_control_channel},
    clocks::Clocks,
    color::Hsluv,
    control::{ControlMessage, Controller, MetaCommand, meta_command_from_osc},
    dmx::DmxBuffer,
    fixture::{
        Patch, animation_target::ControllableTargetedAnimation, prelude::FixtureGroupUpdate,
    },
    gui_state::{AnimationSnapshot, DmxPortStatus, PatchSnapshot},
    gui_state::{GuiDirty, SharedGuiState},
    master::MasterControls,
    midi::{EmitMidiChannelMessage, MidiControlMessage, MidiHandler},
    osc::{OscControlMessage, ScopedControlEmitter},
    preview::Previewer,
};

pub use crate::channel::ChannelId;
use tunnels::audio::AudioInput;

use anyhow::{Context, Result, bail};
use color_organ::{HsluvColor, IgnoreEmitter};
use log::error;
use rust_dmx::{DmxPort, OfflineDmxPort};

pub struct Show {
    controller: Controller,
    dmx_ports: Vec<Box<dyn DmxPort>>,
    dmx_buffers: Vec<DmxBuffer>,
    patch: Patch,
    patch_file_path: PathBuf,
    channels: Channels,
    master_controls: MasterControls,
    animation_ui_state: AnimationUIState,
    clocks: Clocks,
    preview: Previewer,
    master_strobe_channel: Option<usize>,
    gui_state: SharedGuiState,
}

const CONTROL_TIMEOUT: Duration = Duration::from_micros(500);
/// The enttec hypothetically outputs 40 fps. This seems to only truly be the
/// case when no writes are being performed. Writing at the port framerate (or
/// even twice as fast) seems to bring the framerate down a bit - adding about
/// 300 us or so in between frames. Thus this slightly odd update interval.
pub const UPDATE_INTERVAL: Duration = Duration::from_micros(25300);

impl Show {
    pub fn new(
        patch: Patch,
        patch_file_path: PathBuf,
        controller: Controller,
        dmx_ports: Vec<Box<dyn DmxPort>>,
        clocks: Clocks,
        preview: Previewer,
        gui_state: SharedGuiState,
    ) -> Result<Self> {
        let channels = Channels::from_iter(patch.channels().cloned());
        let initial_channel = channels.current_channel();
        let animation_ui_state = AnimationUIState::new(initial_channel);

        let mut show = Self {
            controller,
            dmx_buffers: vec![[0u8; 512]; dmx_ports.len()],
            dmx_ports,
            patch,
            patch_file_path,
            channels,
            master_controls: Default::default(),
            animation_ui_state,
            clocks,
            preview,
            master_strobe_channel: None,
            gui_state,
        };
        show.reconcile_submaster_wings()?;
        show.reconcile_clock_wing()?;
        show.refresh_ui();
        show.snapshot_gui_state(GuiDirty::all());
        // Populate initial patch snapshot for the GUI.
        if let Ok(groups) = crate::fixture::patch::parse_file(&show.patch_file_path) {
            show.gui_state
                .patch_snapshot
                .store(Arc::new(PatchSnapshot { groups }));
        }
        Ok(show)
    }

    /// Run the show forever in the current thread.
    pub fn run(&mut self) {
        let mut last_update = Instant::now();

        loop {
            // Process a control event if one is pending.
            match self.control(CONTROL_TIMEOUT) {
                Ok(dirty) => {
                    if !dirty.is_empty() {
                        self.snapshot_gui_state(dirty);
                    }
                }
                Err(err) => error!("A control error occurred: {err:#}."),
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
    fn control(&mut self, timeout: Duration) -> Result<GuiDirty> {
        let msg = match self.controller.recv(timeout)? {
            Some(m) => m,
            None => {
                return Ok(GuiDirty::CLEAN);
            }
        };

        match msg {
            ControlMessage::MidiDeviceChange(change) => {
                let needs_ui_refresh = self.controller.handle_device_change(change)?;
                if needs_ui_refresh {
                    self.refresh_ui();
                }
                Ok(GuiDirty::MIDI_SLOTS)
            }
            ControlMessage::Midi(msg) => {
                self.handle_midi_message(&msg)?;
                Ok(GuiDirty::CLEAN)
            }
            ControlMessage::Osc(msg) => self.handle_osc_message(&msg),
            ControlMessage::Meta(cmd, reply) => {
                let result = self.handle_meta_command(cmd);
                if let Some(reply) = reply {
                    let _ = reply.send(result.as_ref().map(|_| ()).map_err(|e| format!("{e:#}")));
                }
                result
            }
        }
    }

    /// Handle a meta-command.
    fn handle_meta_command(&mut self, cmd: MetaCommand) -> Result<GuiDirty> {
        match cmd {
            MetaCommand::ReloadPatch => {
                let groups = crate::fixture::patch::parse_file(&self.patch_file_path)?;
                self.patch.repatch(&groups)?;
                self.gui_state
                    .patch_snapshot
                    .store(Arc::new(PatchSnapshot { groups }));
                self.post_repatch()
            }
            MetaCommand::Repatch(groups) => {
                self.patch.repatch(&groups)?;
                self.gui_state
                    .patch_snapshot
                    .store(Arc::new(PatchSnapshot { groups }));
                self.post_repatch()
            }
            MetaCommand::RefreshUI => {
                self.refresh_ui();
                Ok(GuiDirty::all())
            }
            MetaCommand::ResetAllAnimations => {
                for group in self.patch.iter_mut() {
                    group.reset_animations();
                }
                self.refresh_ui();
                Ok(GuiDirty::CLEAN)
            }
            MetaCommand::AssignDmxPort { universe, port } => {
                assign_dmx_port(&mut self.dmx_ports, &mut self.dmx_buffers, universe, port)?;
                Ok(GuiDirty::DMX_PORTS)
            }
            MetaCommand::AddMidiDevice(spec) => {
                self.controller.add_midi_device(spec)?;
                self.refresh_ui();
                Ok(GuiDirty::MIDI_SLOTS)
            }
            MetaCommand::ClearMidiDevice { slot_name } => {
                self.controller.clear_midi_device(&slot_name)?;
                self.refresh_ui();
                Ok(GuiDirty::MIDI_SLOTS)
            }
            MetaCommand::ConnectMidiPort {
                slot_name,
                device_id,
                kind,
            } => {
                self.controller
                    .connect_midi_port(&slot_name, device_id, kind)?;
                self.refresh_ui();
                Ok(GuiDirty::MIDI_SLOTS)
            }
            MetaCommand::UseClockService(service) => {
                self.clocks = Clocks::Service(service);
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(GuiDirty::MIDI_SLOTS | GuiDirty::CLOCK_STATE)
            }
            MetaCommand::UseInternalClocks(device_name) => {
                let audio_input = device_name
                    .map(|name| AudioInput::new(Some(name)))
                    .transpose()?;
                self.clocks = Clocks::internal(audio_input);
                self.reconcile_clock_wing()?;
                self.refresh_ui();
                Ok(GuiDirty::MIDI_SLOTS | GuiDirty::CLOCK_STATE)
            }
            MetaCommand::RegisterOscClient(client_id) => {
                println!("Registering new OSC client at {client_id}.");
                self.controller.register_osc_client(client_id);
                self.refresh_ui();
                Ok(GuiDirty::CLEAN)
            }
            MetaCommand::DropOscClient(client_id) => {
                println!("Deregistering OSC client at {client_id}.");
                self.controller.deregister_osc_client(client_id);
                Ok(GuiDirty::CLEAN)
            }
            MetaCommand::SetMasterStrobeChannel(enable) => {
                if enable {
                    let ch = self.resolve_strobe_channel().context(
                        "cannot enable master strobe: last wing fader is occupied by a fixture group",
                    )?;
                    self.set_master_strobe_channel(Some(ch));
                } else {
                    self.set_master_strobe_channel(None);
                }
                Ok(GuiDirty::CLEAN)
            }
        }
    }

    /// Shared post-repatch logic: clear channels, reconcile wings, resize DMX buffers.
    fn post_repatch(&mut self) -> Result<GuiDirty> {
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
        if self.master_strobe_channel.is_some() {
            // Re-resolve: channel may have moved to a new wing or become occupied.
            self.set_master_strobe_channel(self.resolve_strobe_channel());
        }
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
        Ok(GuiDirty::MIDI_SLOTS | GuiDirty::DMX_PORTS)
    }

    /// Returns `Some(channel_index)` if the last wing fader is available for
    /// strobe, `None` if it is occupied by a fixture group.
    fn resolve_strobe_channel(&self) -> Option<usize> {
        let ch = strobe_control_channel(self.channels.channel_count());
        if self.channels.validate_channel(ch).is_ok() {
            None // channel is occupied by a fixture group
        } else {
            Some(ch) // available
        }
    }

    /// Update the strobe channel state and sync to GUI.
    fn set_master_strobe_channel(&mut self, value: Option<usize>) {
        self.master_strobe_channel = value;
        self.gui_state
            .master_strobe_fader_channel_mapped
            .store(value.is_some(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Handle a channel control message, routing to strobe or fixture groups.
    fn handle_channel_message(
        &mut self,
        msg: &crate::channel::ControlMessage,
    ) -> Result<()> {
        let sender = self.controller.sender_with_metadata(None);
        if let Some(strobe_ch) = self.master_strobe_channel
            && let crate::channel::ControlMessage::Control { channel_id, msg } = msg
            && *channel_id == Some(strobe_ch)
        {
            self.master_controls.handle_strobe_channel(msg, &sender);
        } else {
            self.channels.control(
                msg,
                &mut self.patch,
                &self.animation_ui_state,
                &sender,
            )?;
        }
        Ok(())
    }

    /// Handle a single MIDI control message.
    fn handle_midi_message(&mut self, msg: &MidiControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(None);
        let Some(show_ctrl_msg) = msg.device.interpret(&msg.event) else {
            return Ok(());
        };
        match show_ctrl_msg {
            ShowControlMessage::Channel(msg) => {
                self.handle_channel_message(&msg)?;
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
    fn handle_osc_message(&mut self, msg: &OscControlMessage) -> Result<GuiDirty> {
        let sender = self.controller.sender_with_metadata(Some(&msg.client_id));

        match msg.group() {
            "Meta" => {
                if let Some(cmd) = meta_command_from_osc(msg)? {
                    self.handle_meta_command(cmd)
                } else {
                    Ok(GuiDirty::CLEAN)
                }
            }
            crate::master::GROUP => {
                self.master_controls.control_osc(msg, &sender)?;
                Ok(GuiDirty::CLEAN)
            }
            crate::osc::channels::GROUP => {
                self.channels.control_osc(
                    msg,
                    &mut self.patch,
                    &self.animation_ui_state,
                    &sender,
                )?;
                Ok(GuiDirty::CLEAN)
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
                )?;
                Ok(GuiDirty::CLEAN)
            }
            crate::osc::audio::GROUP => {
                self.clocks.control_audio_osc(msg, &mut self.controller)?;
                Ok(GuiDirty::CLEAN)
            }
            crate::osc::clock::GROUP => {
                self.clocks.control_clock_osc(msg, &mut self.controller)?;
                Ok(GuiDirty::CLEAN)
            }
            // Assume any other control group is referring to a fixture group.
            fixture_group => {
                self.patch.get_mut(fixture_group)?.control(
                    msg,
                    ChannelStateEmitter::new(
                        self.channels.channel_for_fixture(fixture_group),
                        &sender,
                    ),
                )?;
                Ok(GuiDirty::CLEAN)
            }
        }
    }

    /// Selectively snapshot GUI state for the dirty domains.
    fn snapshot_gui_state(&self, dirty: GuiDirty) {
        if dirty.contains(GuiDirty::MIDI_SLOTS) {
            self.gui_state
                .midi_slots
                .store(Arc::new(self.controller.midi_slot_statuses()));
        }
        if dirty.contains(GuiDirty::CLOCK_STATE) {
            self.gui_state
                .clock_status
                .store(Arc::new(self.clocks.status()));
        }
        if dirty.contains(GuiDirty::DMX_PORTS) {
            self.gui_state.dmx_port_status.store(Arc::new(
                DmxPortStatus {
                    ports: self.dmx_ports.iter().map(|p| p.to_string()).collect(),
                },
            ));
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

        if let Err(err) = self.snapshot_animation_state() {
            error!("Animation state snapshot error: {err}.");
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

    /// Snapshot animation state into the shared GUI state, if the visualizer is active.
    fn snapshot_animation_state(&mut self) -> Result<()> {
        if !self
            .gui_state
            .visualizer_active
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Ok(());
        }
        let Some(current_channel) = self.channels.current_channel() else {
            return Ok(());
        };
        let group = self
            .channels
            .group_by_channel(&self.patch, current_channel)?;
        let animation_index = self
            .animation_ui_state
            .animation_index_for_channel(current_channel);
        self.gui_state
            .animation_state
            .store(Arc::new(AnimationSnapshot {
                animation: group
                    .get_animation(animation_index)
                    .map(ControllableTargetedAnimation::anim)
                    .cloned()
                    .unwrap_or_default(),
                clocks: self.clocks.get(),
                fixture_count: group.fixture_configs().len(),
            }));
        Ok(())
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
    // Prevent assigning the same port to multiple universes.
    let new_name = port.to_string();
    let offline_name = OfflineDmxPort.to_string();
    if new_name != offline_name {
        for (i, existing) in dmx_ports.iter().enumerate() {
            if i != universe && existing.to_string() == new_name {
                bail!("port {new_name} is already assigned to universe {i}");
            }
        }
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
            std::fs::write(
                &patch_path,
                "- fixture: Dimmer\n  patches:\n    - addr: 1\n",
            )
            .unwrap();
            let patch = Patch::from_file(&patch_path).unwrap();
            let (mut show, send) = Show::test_new(patch, patch_path);

            // Send the client handle back to the calling thread.
            send_tx
                .send(CommandClient::new(send, zmq::Context::new()))
                .unwrap();

            // Process commands until the client is dropped.
            loop {
                match show.control(Duration::from_millis(100)) {
                    Ok(_dirty) => {}
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
        let clocks = Clocks::test_new();
        let initial_clock_status = clocks.status();
        let gui_state: SharedGuiState = Arc::new(crate::gui_state::GuiState::new(
            vec![],
            initial_clock_status,
            String::new(),
            controller.osc_client_listener(),
        ));
        let mut show = Self {
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
            clocks,
            preview: Previewer::Off,
            master_strobe_channel: None,
            gui_state,
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to open port")
        );
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
    fn assign_dmx_port_rejects_duplicate() {
        let (mut show, _dir) = show_from_yaml(TWO_UNIVERSE_PATCH);

        // Assign a mock port to universe 0.
        show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 0,
            port: Box::new(MockDmxPort::new()),
        })
        .unwrap();

        // Assigning the same port type to universe 1 should fail.
        let result = show.handle_meta_command(MetaCommand::AssignDmxPort {
            universe: 1,
            port: Box::new(MockDmxPort::new()),
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already assigned"));
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

    #[test]
    fn snapshot_animation_state_skipped_when_inactive() {
        let (mut show, _dir) = show_from_yaml(ONE_UNIVERSE_PATCH);

        // Visualizer is inactive by default.
        assert!(
            !show
                .gui_state
                .visualizer_active
                .load(std::sync::atomic::Ordering::Relaxed)
        );

        show.snapshot_animation_state().unwrap();

        // Animation state should still be the default (fixture_count == 0).
        let state = show.gui_state.animation_state.load();
        assert_eq!(state.fixture_count, 0);
    }

    #[test]
    fn snapshot_animation_state_updates_when_active() {
        let (mut show, _dir) = show_from_yaml(ONE_UNIVERSE_PATCH);

        // Activate the visualizer.
        show.gui_state
            .visualizer_active
            .store(true, std::sync::atomic::Ordering::Relaxed);

        show.snapshot_animation_state().unwrap();

        // The patch has one Dimmer fixture, so fixture_count should be 1.
        let state = show.gui_state.animation_state.load();
        assert_eq!(state.fixture_count, 1);
    }

    /// Generate YAML for N dimmer fixtures, each on a separate channel.
    fn n_dimmer_yaml(n: usize) -> String {
        (0..n)
            .map(|i| {
                format!(
                    "- fixture: Dimmer\n  group: dimmer_{i}\n  patches:\n    - addr: {}\n",
                    i + 1
                )
            })
            .collect()
    }

    #[test]
    fn master_strobe_channel_routing() {
        // 1 dimmer = 1 channel, 1 wing, strobe on fader 7.
        let (mut show, _dir) = show_from_yaml(ONE_UNIVERSE_PATCH);
        let strobe_ch = strobe_control_channel(show.channels.channel_count());
        assert_eq!(strobe_ch, 7);

        // Default strobe intensity is 1.0 (full).
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::ONE
        );

        // Disabled by default: level to strobe fader goes to fixture, not strobe.
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Level(
                number::UnipolarFloat::new(0.5),
            ),
        })
        // Channel 7 has no fixture group, so this is an out-of-range error.
        .ok();
        // Intensity unchanged — message didn't route to strobe.
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::ONE
        );

        // Enable strobe.
        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Level routes to strobe intensity.
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Level(
                number::UnipolarFloat::new(0.75),
            ),
        })
        .unwrap();
        assert_eq!(
            show.master_controls.strobe().intensity(),
            number::UnipolarFloat::new(0.75)
        );

        // Knob 0 routes to strobe rate.
        let initial_rate = show.master_controls.strobe().rate_control();
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(strobe_ch),
            msg: crate::channel::ChannelControlMessage::Knob {
                index: 0,
                value: crate::channel::KnobValue::Unipolar(
                    number::UnipolarFloat::new(0.5),
                ),
            },
        })
        .unwrap();
        assert_ne!(show.master_controls.strobe().rate_control(), initial_rate);

        // Non-strobe fader (channel 0) doesn't affect strobe.
        let intensity_before = show.master_controls.strobe().intensity();
        show.handle_channel_message(&crate::channel::ControlMessage::Control {
            channel_id: Some(0),
            msg: crate::channel::ChannelControlMessage::Level(
                number::UnipolarFloat::new(1.0),
            ),
        })
        .unwrap();
        assert_eq!(
            show.master_controls.strobe().intensity(),
            intensity_before
        );
    }

    #[test]
    fn enable_strobe_fails_when_fader_occupied() {
        // 8 dimmers = all 8 faders on wing 1 occupied.
        let yaml = n_dimmer_yaml(8);
        let (mut show, _dir) = show_from_yaml(&yaml);
        assert_eq!(show.channels.channel_count(), 8);

        let result = show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("occupied"));
    }

    #[test]
    fn repatch_auto_disables_strobe_when_fader_occupied() {
        // 7 dimmers = fader 7 available for strobe.
        let yaml = n_dimmer_yaml(7);
        let (mut show, dir) = show_from_yaml(&yaml);

        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Repatch to 8 dimmers — fader 7 now occupied.
        let yaml_8 = n_dimmer_yaml(8);
        std::fs::write(dir.path().join("patch.yaml"), &yaml_8).unwrap();
        show.handle_meta_command(MetaCommand::ReloadPatch).unwrap();

        assert_eq!(show.master_strobe_channel, None);
        assert!(
            !show
                .gui_state
                .master_strobe_fader_channel_mapped
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }

    #[test]
    fn repatch_moves_strobe_to_new_wing() {
        // 7 dimmers = 1 wing, strobe on fader 7.
        let yaml = n_dimmer_yaml(7);
        let (mut show, dir) = show_from_yaml(&yaml);

        show.handle_meta_command(MetaCommand::SetMasterStrobeChannel(true))
            .unwrap();
        assert_eq!(show.master_strobe_channel, Some(7));

        // Repatch to 9 dimmers — 2 wings, strobe moves to fader 15.
        let yaml_9 = n_dimmer_yaml(9);
        std::fs::write(dir.path().join("patch.yaml"), &yaml_9).unwrap();
        show.handle_meta_command(MetaCommand::ReloadPatch).unwrap();

        assert_eq!(show.master_strobe_channel, Some(15));
        assert!(
            show.gui_state
                .master_strobe_fader_channel_mapped
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    }
}
