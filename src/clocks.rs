use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Result;
use tunnels::{
    audio::{AudioFrame, AudioInput, AudioSnapshot, AudioState, EnvelopeStreams, Role},
    clock_bank::{ClockBank, ControlMessage},
    clock_server::SharedClockData,
};

use crate::{
    clock_service::ClockService,
    control::Controller,
    gui_state::{ClockStatus, StateDirty},
    osc::{GroupControlMap, OscControlMessage},
};

#[allow(clippy::large_enum_variant)]
pub enum Clocks {
    /// Clocks and audio frames from a remote clock service.
    Service(ClockService),
    /// Local control of clocks with local audio input.
    Internal {
        clocks: ClockBank,
        clock_controls: GroupControlMap<tunnels::clock_bank::ControlMessage>,
        audio_input: AudioInput,
        /// The audio input's latest frame.
        frame: AudioFrame,
    },
}

#[cfg(test)]
impl Clocks {
    pub fn test_new() -> Self {
        Clocks::Service(ClockService::test_new())
    }

    /// Internal clocks whose audio input reads its frames from `frames`.
    pub fn test_internal_with_frames(frames: tunnels::audio::frame_buffer::FrameReader) -> Self {
        let mut clock_controls = GroupControlMap::default();
        crate::osc::clock::map_controls(&mut clock_controls);
        Clocks::Internal {
            clocks: ClockBank::default(),
            clock_controls,
            audio_input: AudioInput::from_frames(frames),
            frame: AudioFrame::default(),
        }
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
        let audio_input = AudioInput::new(audio_device_name, envelope_streams_tx)?;
        Ok(Clocks::Internal {
            clocks,
            clock_controls,
            audio_input,
            frame: AudioFrame::default(),
        })
    }

    /// Snapshot of audio input state, when running in Internal mode.
    pub fn audio_snapshot(&self) -> Option<AudioSnapshot> {
        match self {
            Self::Internal { audio_input, .. } => Some(audio_input.snapshot()),
            Self::Service(_) => None,
        }
    }

    /// The clocks' state and the latest audio frame, following `active_role`
    /// in it. A service's frame is followed by `active_role` too, whatever
    /// role the service follows.
    pub fn get(&self, active_role: Role) -> SharedClockData {
        match self {
            Self::Service(service) => {
                let data = service.get();
                SharedClockData {
                    clock_bank: data.clock_bank,
                    audio: AudioState {
                        frame: data.audio.frame,
                        active_role,
                    },
                }
            }
            Self::Internal { clocks, frame, .. } => SharedClockData {
                clock_bank: clocks.as_static(),
                audio: AudioState {
                    frame: *frame,
                    active_role,
                },
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

    /// Handle an audio control message.
    pub fn control_audio(
        &mut self,
        msg: tunnels::audio::ControlMessage,
        emitter: &mut Controller,
    ) -> StateDirty {
        let Self::Internal { audio_input, .. } = self else {
            return StateDirty::CLEAN;
        };
        audio_input.control(msg, emitter);
        StateDirty::AUDIO
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

    /// Advance internal clocks by `delta_t`, reading the audio input's latest
    /// frame and following `active_role` in it. Clocks from a service advance
    /// in the service and are left alone.
    pub fn update(&mut self, delta_t: Duration, active_role: Role, controller: &mut Controller) {
        let Self::Internal {
            clocks,
            audio_input,
            frame,
            ..
        } = self
        else {
            return;
        };
        *frame = audio_input.frame();
        let audio = AudioState {
            frame: *frame,
            active_role,
        };
        audio_input.update_state(delta_t, audio.envelope(), controller);
        clocks.update_state(delta_t, &audio, controller);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tunnels::audio::UnipolarF32;

    fn internal_clocks() -> Clocks {
        let (tx, _rx) = std::sync::mpsc::channel();
        Clocks::internal(None, tx).expect("internal clocks should construct in test")
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
            StateDirty::CLEAN,
        );

        let mut internal = internal_clocks();
        assert_eq!(
            internal.control_audio(
                tunnels::audio::ControlMessage::ResetParameters,
                &mut controller,
            ),
            StateDirty::AUDIO,
        );
    }

    /// Internal clocks carry the audio input's latest frame, and clocks from
    /// a service carry the service's frame; either follows the role it is
    /// asked for, never the service's own.
    #[test]
    fn clock_data_carries_the_frame_and_the_followed_role() {
        let (mut controller, _send, _osc_recv) = Controller::test_new();
        let frame = AudioFrame::new(
            std::array::from_fn(|r| UnipolarF32::new(0.1 + 0.2 * r as f32)),
            std::array::from_fn(|b| UnipolarF32::new((b + 1) as f32 / 27.0)),
        );
        let (mut producer, reader) = tunnels::audio::frame_buffer::frame_buffer();
        let mut internal = Clocks::test_internal_with_frames(reader);
        producer.publish(frame);

        assert_eq!(
            internal.get(Role::Hats).audio,
            AudioState {
                frame: AudioFrame::default(),
                active_role: Role::Hats,
            },
            "no frame is read before the first update",
        );
        for role in [Role::Hats, Role::Kick] {
            internal.update(Duration::from_millis(25), role, &mut controller);
            let audio = internal.get(role).audio;
            assert_eq!(
                audio,
                AudioState {
                    frame,
                    active_role: role,
                },
            );
            assert_eq!(audio.envelope(), frame.role(role));
        }

        let mut service = Clocks::Service(ClockService::test_with(SharedClockData {
            clock_bank: Default::default(),
            audio: AudioState {
                frame,
                active_role: Role::Mid,
            },
        }));
        service.update(Duration::from_millis(25), Role::Hats, &mut controller);
        let audio = service.get(Role::Hats).audio;
        assert_eq!(
            audio,
            AudioState {
                frame,
                active_role: Role::Hats,
            },
        );
        assert_eq!(audio.envelope(), frame.role(Role::Hats));
    }

    #[test]
    fn audio_snapshot_present_only_in_internal_mode() {
        assert!(Clocks::test_new().audio_snapshot().is_none());
        assert!(internal_clocks().audio_snapshot().is_some());
    }
}
