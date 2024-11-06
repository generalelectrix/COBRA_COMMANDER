use std::time::{Duration, Instant};

use crate::{
    animation::AnimationUIState,
    channel::{ChannelStateEmitter, Channels},
    clock_service::ClockService,
    config::Config,
    control::{ControlMessage, Controller},
    dmx::DmxBuffer,
    fixture::{FixtureGroupKey, GroupName, Patch},
    master::MasterControls,
    midi::{MidiControlMessage, MidiHandler},
    osc::{GroupControlMap, OscControlMessage, ScopedControlEmitter},
    wled::WledResponse,
};

pub use crate::channel::ChannelId;
use anyhow::{bail, Result};
use log::error;
use number::UnipolarFloat;
use rust_dmx::DmxPort;
use tunnels::{
    audio::AudioInput,
    clock_bank::ClockBank,
    clock_server::{SharedClockData, StaticClockBank},
};

pub struct Show {
    controller: Controller,
    patch: Patch,
    channels: Channels,
    master_controls: MasterControls,
    animation_ui_state: AnimationUIState,
    clocks: Clocks,
}

#[allow(clippy::large_enum_variant)]
pub enum Clocks {
    Service(ClockService),
    Internal {
        clocks: ClockBank,
        audio_input: AudioInput,
        audio_controls: GroupControlMap<tunnels::audio::ControlMessage>,
    },
}

impl Clocks {
    pub fn get(&self) -> SharedClockData {
        match self {
            Self::Service(service) => service.get(),
            Self::Internal {
                clocks,
                audio_input,
                ..
            } => SharedClockData {
                clock_bank: StaticClockBank(clocks.as_static()),
                audio_envelope: audio_input.envelope(),
            },
        }
    }

    /// Emit all current audio and clock state.
    pub fn emit_state(&self, emitter: &mut Controller) {
        let Self::Internal {
            clocks,
            audio_input,
            ..
        } = self
        else {
            return;
        };
        audio_input.emit_state(emitter);
        clocks.emit_state(emitter);
    }

    /// Update clock state.
    pub fn update(&mut self, delta_t: Duration, controller: &mut Controller) {
        if let Clocks::Internal {
            clocks,
            audio_input,
            ..
        } = self
        {
            audio_input.update_state(delta_t, controller);
            let audio_envelope = audio_input.envelope();
            clocks.update_state(delta_t, audio_envelope, controller);
        }
    }
}

const CONTROL_TIMEOUT: Duration = Duration::from_millis(1);
const UPDATE_INTERVAL: Duration = Duration::from_millis(20);

impl Show {
    pub fn new(cfg: Config, clocks: Clocks) -> Result<Self> {
        let mut channels = Channels::new();
        let mut patch = Patch::default();

        let controller = Controller::from_config(&cfg)?;

        for fixture in cfg.fixtures.into_iter() {
            patch.patch(&mut channels, fixture)?;
        }

        let master_controls = MasterControls::new();
        let initial_channel = channels.current_channel();
        let animation_ui_state = AnimationUIState::new(initial_channel);

        let mut show = Self {
            controller,
            patch,
            channels,
            master_controls,
            animation_ui_state,
            clocks,
        };
        show.refresh_ui()?;
        Ok(show)
    }

    /// Return the number of universes patched in the show.
    pub fn universe_count(&self) -> usize {
        self.patch.universe_count()
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
        let Some(channel_ctrl_msg) = msg.device.interpret(&msg.event) else {
            return Ok(());
        };
        match channel_ctrl_msg {
            ShowControlMessage::Channel(msg) => {
                self.channels
                    .control(&msg, &mut self.patch, &self.animation_ui_state, &sender)
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
        }
    }

    /// Handle a single OSC message.
    fn handle_osc_message(&mut self, msg: &OscControlMessage) -> Result<()> {
        let sender = self.controller.sender_with_metadata(Some(&msg.client_id));

        match msg.entity_type() {
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
            crate::osc::audio::GROUP => {
                let Clocks::Internal {
                    audio_input,
                    audio_controls,
                    ..
                } = &mut self.clocks
                else {
                    bail!("cannot handle audio control message because no audio input is configured\n{msg:?}");
                };
                let Some((msg, _talkback)) = audio_controls.handle(msg)? else {
                    return Ok(());
                };
                audio_input.control(msg, &mut self.controller);
                Ok(())
            }
            // Assume any other group is the name of a fixture.
            fixture_type => {
                let Some(fixture_type) = self.patch.lookup_fixture_type(fixture_type) else {
                    bail!(
                        "entity type \"{}\" not registered with patch, from OSC message {msg:?}",
                        msg.entity_type()
                    );
                };
                let group_key = FixtureGroupKey {
                    fixture: fixture_type,
                    group: msg.group().map(GroupName::new),
                };
                self.patch.get_mut(&group_key)?.control(
                    msg,
                    ChannelStateEmitter::new(
                        self.channels.channel_for_fixture(&group_key),
                        &sender,
                    ),
                )
            }
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
        for group in self.patch.iter() {
            group.emit_state(ChannelStateEmitter::new(
                self.channels.channel_for_fixture(group.key()),
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
            )?;
        }

        self.clocks.emit_state(&mut self.controller);

        Ok(())
    }
}

/// Strongly-typed top-level show control messages.
/// These cover all of the fixed control features, but not fixture-specific controls.
#[derive(Debug, Clone)]
pub enum ShowControlMessage {
    Master(crate::master::ControlMessage),
    Channel(crate::channel::ControlMessage),
    Animation(crate::animation::ControlMessage),
}
