use std::time::{Duration, Instant};

use crate::{
    animation::AnimationUIState,
    animation_visualizer::{AnimationPublisher, AnimationServiceState},
    channel::{ChannelStateEmitter, Channels},
    clocks::Clocks,
    color::Hsluv,
    control::{ControlMessage, Controller},
    dmx::DmxBuffer,
    fixture::{animation_target::ControllableTargetedAnimation, Patch},
    master::MasterControls,
    midi::{MidiControlMessage, MidiHandler},
    osc::{OscControlMessage, ScopedControlEmitter},
    wled::WledResponse,
};

pub use crate::channel::ChannelId;
use anyhow::{bail, Result};
use color_organ::{HsluvColor, IgnoreEmitter};
use log::error;
use number::UnipolarFloat;
use rust_dmx::DmxPort;

pub struct Show {
    controller: Controller,
    patch: Patch,
    channels: Channels,
    master_controls: MasterControls,
    animation_ui_state: AnimationUIState,
    clocks: Clocks,
    animation_service: Option<AnimationPublisher>,
}

const CONTROL_TIMEOUT: Duration = Duration::from_millis(1);
const UPDATE_INTERVAL: Duration = Duration::from_millis(20);

impl Show {
    pub fn new(
        mut patch: Patch,
        controller: Controller,
        clocks: Clocks,
        animation_service: Option<AnimationPublisher>,
    ) -> Result<Self> {
        let channels = Channels::from_iter(patch.channels().cloned());

        let master_controls = MasterControls::new();
        let initial_channel = channels.current_channel();
        let animation_ui_state = AnimationUIState::new(initial_channel);

        patch.initialize_color_organs();

        let mut show = Self {
            controller,
            patch,
            channels,
            master_controls,
            animation_ui_state,
            clocks,
            animation_service,
        };
        show.refresh_ui()?;
        Ok(show)
    }

    /// Run the show forever in the current thread.
    pub fn run(&mut self, dmx_ports: &mut [Box<dyn DmxPort>]) {
        let mut last_update = Instant::now();
        let mut dmx_buffers = vec![[0u8; 512]; dmx_ports.len()];
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
                self.render(&mut dmx_buffers);
                for (port, buffer) in dmx_ports.iter_mut().zip(&dmx_buffers) {
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
            ControlMessage::Midi(msg) => self.handle_midi_message(&msg),
            ControlMessage::Osc(msg) => self.handle_osc_message(&msg),
            ControlMessage::Wled(msg) => self.handle_wled_response(&msg),
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
                    .control(&msg, &mut self.patch, &self.animation_ui_state, &sender)
            }
            ShowControlMessage::Clock(msg) => {
                self.clocks.control(msg, sender.controller);
                Ok(())
            }
            ShowControlMessage::Master(msg) => self.master_controls.control(&msg, &sender),
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
                )
            }
            ShowControlMessage::ColorOrgan(msg) => {
                // FIXME: this is really janky and has no way to route messages.
                for group in self.patch.iter_mut() {
                    let Some(color_organ) = group.color_organ_mut() else {
                        continue;
                    };
                    color_organ.control(msg.clone(), &IgnoreEmitter);
                }
                Ok(())
            }
        }
    }

    /// Handle a single OSC message.
    fn handle_osc_message(&mut self, msg: &OscControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(Some(&msg.client_id));

        match msg.group() {
            "Meta" => {
                if msg.control() == "RefreshUI" {
                    if msg.get_bool()? {
                        self.refresh_ui()?;
                    }
                } else {
                    bail!("unknown Meta control {}", msg.control());
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
            crate::osc::audio::GROUP => match &mut self.clocks {
                Clocks::Internal {
                    audio_input,
                    audio_controls,
                    ..
                }
                | Clocks::Mixed {
                    audio_input,
                    audio_controls,
                    ..
                } => {
                    let Some((msg, _talkback)) = audio_controls.handle(msg)? else {
                        return Ok(());
                    };
                    audio_input.control(msg, &mut self.controller);
                    Ok(())
                }
                Clocks::Service(_) => {
                    bail!("cannot handle audio control message because no audio input is configured\n{msg:?}");
                }
            },
            // Assume any other control group is referring to a ficture group.
            fixture_group => self.patch.get_mut(fixture_group)?.control(
                msg,
                ChannelStateEmitter::new(self.channels.channel_for_fixture(fixture_group), &sender),
            ),
        }
    }

    /// Handle a single response from WLED.
    fn handle_wled_response(&mut self, _msg: &WledResponse) -> Result<()> {
        // TODO: decide how to map responses back
        Ok(())
    }

    /// Update the state of the show using the provided timestep.
    fn update(&mut self, delta_t: Duration) {
        self.clocks.update(delta_t, &mut self.controller);
        self.master_controls.update(delta_t);
        for fixture in self.patch.iter_mut() {
            fixture.update(&self.master_controls, delta_t, UnipolarFloat::ZERO);
        }
        let clock_state = self.clocks.get();
        self.master_controls.clock_state = clock_state.clock_bank;
        self.master_controls.audio_envelope = clock_state.audio_envelope;
        if let Err(err) = self.publish_animation_state() {
            error!("Animation state publishing error: {err}.");
        };
    }

    /// Render the state of the show out to DMX.
    fn render(&self, dmx_buffers: &mut [DmxBuffer]) {
        // NOTE: we don't bother to empty the buffer because we will always
        // overwrite all previously-rendered state.
        for group in self.patch.iter() {
            group.render(&self.master_controls, dmx_buffers);
        }
    }

    /// Send messages to refresh all UI state.
    fn refresh_ui(&mut self) -> anyhow::Result<()> {
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
            self.animation_ui_state.emit_state(
                current_channel,
                self.channels
                    .group_by_channel(&self.patch, current_channel)?,
                &ScopedControlEmitter {
                    entity: crate::osc::animation::GROUP,
                    emitter,
                },
            );
        }

        self.clocks.emit_state(&mut self.controller);

        Ok(())
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
    ColorOrgan(color_organ::ControlMessage<Hsluv>),
}

impl From<Hsluv> for HsluvColor {
    fn from(c: Hsluv) -> Self {
        Self::new(c.hue, c.sat, c.lightness)
    }
}
