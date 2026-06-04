use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Result;
use tunnels::{
    audio::{AudioInput, AudioSnapshot, EnvelopeStreams},
    clock_bank::{ClockBank, ControlMessage},
    clock_server::{SharedClockData, StaticClockBank},
};

use crate::{
    clock_service::ClockService,
    control::Controller,
    gui_state::{ClockStatus, GuiDirty},
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

#[cfg(test)]
impl Clocks {
    pub fn test_new() -> Self {
        Clocks::Service(ClockService::test_new())
    }
}

impl Clocks {
    /// Return true if these are internally-controlled clocks.
    pub fn is_internal(&self) -> bool {
        match self {
            Self::Internal { .. } => true,
            Self::Service(_) => false,
        }
    }

    /// Return the current clock status for GUI display.
    pub fn status(&self) -> ClockStatus {
        match self {
            Self::Service(service) => ClockStatus::Remote {
                provider: service.provider().to_string(),
            },
            Self::Internal { audio_input, .. } => ClockStatus::Internal {
                audio_device: audio_input.device_name().to_string(),
            },
        }
    }

    /// Initialize internally-controlled clocks. Opens the named audio device, or
    /// uses an offline device when `audio_device_name` is `None`.
    pub fn internal(
        audio_device_name: Option<String>,
        envelope_streams_tx: Sender<EnvelopeStreams>,
    ) -> Result<Self> {
        let clocks = ClockBank::default();
        let mut clock_controls = GroupControlMap::default();
        crate::osc::clock::map_controls(&mut clock_controls);
        let mut audio_controls = GroupControlMap::default();
        crate::osc::audio::map_controls(&mut audio_controls);
        let audio_input = AudioInput::new(audio_device_name, envelope_streams_tx)?;
        Ok(Clocks::Internal {
            clocks,
            clock_controls,
            audio_input,
            audio_controls,
        })
    }

    /// Snapshot of audio input state, when running in Internal mode.
    pub fn audio_snapshot(&self) -> Option<AudioSnapshot> {
        match self {
            Self::Internal { audio_input, .. } => Some(audio_input.snapshot()),
            Self::Service(_) => None,
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
    ) -> Result<GuiDirty> {
        let Self::Internal {
            audio_input,
            audio_controls,
            ..
        } = self
        else {
            return Ok(GuiDirty::CLEAN);
        };
        let Some((msg, _talkback)) = audio_controls.handle(msg)? else {
            return Ok(GuiDirty::CLEAN);
        };
        audio_input.control(msg, emitter);
        Ok(GuiDirty::AUDIO)
    }

    /// Handle an audio control message.
    pub fn control_audio(
        &mut self,
        msg: tunnels::audio::ControlMessage,
        emitter: &mut Controller,
    ) -> GuiDirty {
        let Self::Internal { audio_input, .. } = self else {
            return GuiDirty::CLEAN;
        };
        audio_input.control(msg, emitter);
        GuiDirty::AUDIO
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::OscClientId;
    use rosc::{OscMessage, OscType};

    fn internal_clocks() -> Clocks {
        let (tx, _rx) = std::sync::mpsc::channel();
        Clocks::internal(None, tx).expect("internal clocks should construct in test")
    }

    fn audio_osc_msg(control: &str, value: f32) -> OscControlMessage {
        OscControlMessage::new(
            OscMessage {
                addr: format!("/Audio/{control}"),
                args: vec![OscType::Float(value)],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    #[test]
    fn control_audio_marks_audio_dirty_only_in_internal_mode() {
        let (mut controller, _send, _osc_recv) = Controller::test_new();

        let mut service = Clocks::test_new();
        assert_eq!(
            service.control_audio(
                tunnels::audio::ControlMessage::ResetParameters,
                &mut controller,
            ),
            GuiDirty::CLEAN,
        );

        let mut internal = internal_clocks();
        assert_eq!(
            internal.control_audio(
                tunnels::audio::ControlMessage::ResetParameters,
                &mut controller,
            ),
            GuiDirty::AUDIO,
        );
    }

    #[test]
    fn control_audio_osc_marks_audio_dirty_only_in_internal_mode() {
        let (mut controller, _send, _osc_recv) = Controller::test_new();
        let msg = audio_osc_msg("FilterCutoff", 0.5);

        assert_eq!(
            Clocks::test_new()
                .control_audio_osc(&msg, &mut controller)
                .expect("recognized msg should not error"),
            GuiDirty::CLEAN,
        );

        assert_eq!(
            internal_clocks()
                .control_audio_osc(&msg, &mut controller)
                .expect("recognized msg should not error"),
            GuiDirty::AUDIO,
        );
    }

    #[test]
    fn audio_snapshot_present_only_in_internal_mode() {
        assert!(Clocks::test_new().audio_snapshot().is_none());
        assert!(internal_clocks().audio_snapshot().is_some());
    }
}
