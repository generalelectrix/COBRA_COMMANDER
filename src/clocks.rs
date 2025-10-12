use std::time::Duration;

use anyhow::{bail, Result};
use tunnels::{
    audio::AudioInput,
    clock_bank::{ClockBank, ControlMessage},
    clock_server::{SharedClockData, StaticClockBank},
};

use crate::{
    clock_service::ClockService,
    control::Controller,
    osc::{GroupControlMap, OscControlMessage},
};

#[allow(clippy::large_enum_variant)]
pub enum Clocks {
    /// Full remote control of clocks and audio envelope.
    Service(ClockService),
    /// Clocks come from the service but the audio input is local.
    Mixed {
        service: ClockService,
        audio_input: AudioInput,
        audio_controls: GroupControlMap<tunnels::audio::ControlMessage>,
    },
    /// No control over clocks. Local audio input.
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
            Self::Mixed {
                service,
                audio_input,
                ..
            } => {
                let clock_bank = service.get().clock_bank;
                SharedClockData {
                    clock_bank,
                    audio_envelope: audio_input.envelope(),
                }
            }
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

    /// Handle a clock control message.
    pub fn control_clock(&mut self, msg: ControlMessage, emitter: &mut Controller) {
        let Self::Internal { clocks, .. } = self else {
            return;
        };
        clocks.control(msg, emitter);
    }

    /// Handle an audio OSC message.
    pub fn control_audio_osc(
        &mut self,
        msg: &OscControlMessage,
        emitter: &mut Controller,
    ) -> Result<()> {
        match self {
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
                audio_input.control(msg, emitter);
                Ok(())
            }
            Clocks::Service(_) => {
                bail!("cannot handle audio control message because no audio input is configured\n{msg:?}");
            }
        }
    }

    /// Handle an audio control message.
    pub fn control_audio(
        &mut self,
        msg: tunnels::audio::ControlMessage,
        emitter: &mut Controller,
    ) -> Result<()> {
        match self {
            Clocks::Internal { audio_input, .. } | Clocks::Mixed { audio_input, .. } => {
                audio_input.control(msg, emitter);
                Ok(())
            }
            Clocks::Service(_) => {
                bail!("cannot handle audio control message because no audio input is configured\n{msg:?}");
            }
        }
    }

    /// Emit all current audio and clock state.
    pub fn emit_state(&self, emitter: &mut Controller) {
        match self {
            Self::Internal {
                clocks,
                audio_input,
                ..
            } => {
                audio_input.emit_state(emitter);
                clocks.emit_state(emitter);
            }
            Self::Mixed { audio_input, .. } => {
                audio_input.emit_state(emitter);
            }
            Self::Service(_) => (),
        }
    }

    /// Update clock state.
    pub fn update(&mut self, delta_t: Duration, controller: &mut Controller) {
        match self {
            Self::Internal {
                clocks,
                audio_input,
                ..
            } => {
                audio_input.update_state(delta_t, controller);
                let audio_envelope = audio_input.envelope();
                clocks.update_state(delta_t, audio_envelope, controller);
            }
            Self::Mixed { audio_input, .. } => {
                audio_input.update_state(delta_t, controller);
                // FIXME: when running in this mode we can't actually use audio
                // envelope to influence clock evolution.
            }
            Self::Service(_) => (),
        }
    }
}
