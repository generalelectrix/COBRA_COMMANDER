//! Show-level controls.

use std::time::Duration;

use tunnels::audio::AudioState;
use tunnels::clock_server::{SharedClockData, StaticClockBank};
use tunnels::spectrum::SpectrumTables;

use crate::fixture::prelude::*;
use crate::osc::ScopedControlEmitter;
use crate::strobe::{Distributor, StrobeClock};

pub struct MasterControls {
    strobe_clock: StrobeClock,
    pub clock_state: StaticClockBank,
    /// The current audio frame and the role followed in it.
    audio: AudioState,
    /// The spectrum of `audio`'s frame, baked for animations to read.
    spectrum: SpectrumTables,
}

impl Default for MasterControls {
    fn default() -> Self {
        let audio = AudioState::default();
        Self {
            strobe_clock: Default::default(),
            clock_state: Default::default(),
            spectrum: SpectrumTables::new(&audio.frame),
            audio,
        }
    }
}

#[cfg(test)]
impl MasterControls {
    /// Construct with a specific strobe clock, for tests.
    pub(crate) fn with_strobe_clock(strobe_clock: StrobeClock) -> Self {
        Self {
            strobe_clock,
            ..Default::default()
        }
    }
}

impl MasterControls {
    /// Take on a frame of clock and audio state, baking the frame's spectrum.
    pub fn set_clock_data(&mut self, data: SharedClockData) {
        self.clock_state = data.clock_bank;
        self.spectrum = SpectrumTables::new(&data.audio.frame);
        self.audio = data.audio;
    }

    /// The current audio frame and the role followed in it.
    pub fn audio(&self) -> &AudioState {
        &self.audio
    }

    /// The spectrum of the current audio frame.
    pub fn spectrum(&self) -> &SpectrumTables {
        &self.spectrum
    }

    pub fn update(&mut self, delta_t: Duration, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        self.strobe_clock.update(delta_t, &self.audio, emitter);
    }

    pub fn emit_state(&self, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        self.strobe_clock.emit_state(emitter);
    }

    pub fn control(&mut self, msg: &ControlMessage, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };

        match msg {
            ControlMessage::Strobe(sc) => {
                self.strobe_clock.control(sc, emitter);
            }
        }
    }

    pub fn handle_strobe_channel(
        &mut self,
        msg: &crate::channel::ChannelControlMessage,
        emitter: &dyn EmitControlMessage,
    ) {
        use crate::channel::ChannelControlMessage::*;
        use crate::strobe::ControlMessage::*;
        use crate::strobe::StateChange::*;
        let strobe_msg = match msg {
            Level(v) => Set(Intensity(*v)),
            Knob { index, value } if *index == 0 => Set(Rate(value.as_unipolar())),
            _ => {
                return;
            }
        };
        self.control(&ControlMessage::Strobe(strobe_msg), emitter);
    }

    pub fn control_osc(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitControlMessage,
    ) -> anyhow::Result<()> {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        // FIXME: need to refactor how GroupControlMap works or lift it up
        // to this level to have more than one receiver...
        self.strobe_clock.control_osc(msg, emitter)
    }

    /// Update and fetch the flash distributor.
    pub fn flash_distributor(&mut self, group_count: usize) -> Distributor {
        self.strobe_clock.distributor(group_count)
    }

    /// Get the strobe clock.
    pub fn strobe(&self) -> &StrobeClock {
        &self.strobe_clock
    }
}

#[derive(Debug, Clone)]
pub enum ControlMessage {
    Strobe(crate::strobe::ControlMessage),
}

#[derive(Debug, Clone)]
pub enum StateChange {
    Strobe(crate::strobe::StateChange),
}

pub const GROUP: &str = "Master";
