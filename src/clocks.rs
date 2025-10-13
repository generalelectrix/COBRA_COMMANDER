use std::time::Duration;

use anyhow::Result;
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
    /// Local control of clocks with local audio input.
    Internal {
        clocks: ClockBank,
        clock_controls: GroupControlMap<tunnels::clock_bank::ControlMessage>,
        audio_input: AudioInput,
        audio_controls: GroupControlMap<tunnels::audio::ControlMessage>,
    },
}

impl Clocks {
    /// Initialize internally-controlled clocks. Use the provided audio input.
    pub fn internal(audio_device: Option<AudioInput>) -> Self {
        let clocks = ClockBank::default();
        let mut clock_controls = GroupControlMap::default();
        crate::osc::clock::map_controls(&mut clock_controls);
        let mut audio_controls = GroupControlMap::default();
        crate::osc::audio::map_controls(&mut audio_controls);
        Clocks::Internal {
            clocks,
            clock_controls,
            audio_input: audio_device.unwrap_or_else(|| AudioInput::new(None).unwrap()),
            audio_controls,
        }
    }

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

    /// Handle a clock OSC message.
    pub fn control_clock_osc(
        &mut self,
        msg: &OscControlMessage,
        emitter: &mut Controller,
    ) -> Result<()> {
        let Self::Internal {
            clocks,
            clock_controls,
            ..
        } = self
        else {
            return Ok(());
        };
        let Some((msg, _talkback)) = clock_controls.handle(msg)? else {
            return Ok(());
        };
        clocks.control(msg, emitter);
        Ok(())
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
        let Self::Internal {
            audio_input,
            audio_controls,
            ..
        } = self
        else {
            return Ok(());
        };
        let Some((msg, _talkback)) = audio_controls.handle(msg)? else {
            return Ok(());
        };
        audio_input.control(msg, emitter);
        Ok(())
    }

    /// Handle an audio control message.
    pub fn control_audio(&mut self, msg: tunnels::audio::ControlMessage, emitter: &mut Controller) {
        let Self::Internal { audio_input, .. } = self else {
            return;
        };
        audio_input.control(msg, emitter);
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
        let Self::Internal {
            clocks,
            audio_input,
            ..
        } = self
        else {
            return;
        };
        audio_input.update_state(delta_t, controller);
        let audio_envelope = audio_input.envelope();
        clocks.update_state(delta_t, audio_envelope, controller);
    }
}
