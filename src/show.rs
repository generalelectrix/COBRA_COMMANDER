use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    animation::AnimationUIState,
    animation_visualizer::{AnimationPublisher, AnimationServiceState},
    channel::{ChannelStateEmitter, Channels},
    clocks::Clocks,
    color::Hsluv,
    control::{ControlMessage, Controller},
    dmx::DmxBuffer,
    fixture::{
        animation_target::ControllableTargetedAnimation, prelude::FixtureGroupUpdate, Patch,
    },
    master::MasterControls,
    midi::{EmitMidiChannelMessage, MidiControlMessage, MidiHandler},
    osc::{OscControlMessage, ScopedControlEmitter},
    preview::Previewer,
};

pub use crate::channel::ChannelId;
use anyhow::{bail, Result};
use color_organ::{HsluvColor, IgnoreEmitter};
use log::error;
use rust_dmx::DmxPort;

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
    animation_service: Option<AnimationPublisher>,
    preview: Previewer,
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
        animation_service: Option<AnimationPublisher>,
        preview: Previewer,
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
            animation_service,
            preview,
        };
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
            ControlMessage::Midi(msg) => self.handle_midi_message(&msg),
            ControlMessage::Osc(msg) => self.handle_osc_message(&msg),
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
                self.channels
                    .control(&msg, &mut self.patch, &self.animation_ui_state, &sender)?;
            }
            ShowControlMessage::Clock(msg) => self.clocks.control_clock(msg, sender.controller),
            ShowControlMessage::Audio(msg) => self.clocks.control_audio(msg, &mut self.controller),
            ShowControlMessage::Master(msg) => {
                self.master_controls.control(&msg, &sender)?;
            }
            ShowControlMessage::Animation(msg) => {
                let Some(channel) = self.channels.current_channel() else {
                    bail!("cannot handle animation control message because no channel is selected\n{msg:?}");
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
                match msg.control() {
                    "ReloadPatch" => {
                        self.patch.repatch_from_file(&self.patch_file_path)?;
                        sender.emit_midi_channel_message(&crate::channel::StateChange::Clear);
                        Channels::emit_osc_state_change(
                            crate::channel::StateChange::Clear,
                            &ScopedControlEmitter {
                                entity: crate::osc::channels::GROUP,
                                emitter: &sender,
                            },
                        );
                        // Re-initialize the channels to match the new patch.
                        self.channels = Channels::from_iter(self.patch.channels().cloned());
                        self.refresh_ui();
                        // Zero out the DMX buffers.
                        for buf in &mut self.dmx_buffers {
                            buf.fill(0);
                        }
                    }
                    "RefreshUI" => {
                        if msg.get_bool()? {
                            self.refresh_ui();
                        }
                    }
                    // TODO: it would be nicer for this to be scoped under Animation,
                    // but that interface is currently tailored to controlling the current group.
                    "ResetAllAnimations" => {
                        for group in self.patch.iter_mut() {
                            group.reset_animations();
                        }
                        // TODO: this is overkill but easiest solution
                        self.refresh_ui();
                    }
                    unknown => {
                        bail!("unknown Meta control {}", unknown)
                    }
                }
                Ok(())
            }
            crate::master::GROUP => self.master_controls.control_osc(msg, &sender),
            crate::osc::channels::GROUP => {
                self.channels
                    .control_osc(msg, &mut self.patch, &self.animation_ui_state, &sender)
            }
            crate::osc::animation::GROUP => {
                let Some(channel) = self.channels.current_channel() else {
                    bail!("cannot handle animation control message because no channel is selected\n{msg:?}");
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
                    flash_now: if group.strobe_enabled() {
                        flash_distributor.next()
                    } else {
                        false
                    },
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
                error!("Refreshing UI: could not get fixture group for current channel {current_channel}.");
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
