//! Show-level controls.

use std::time::Duration;

use number::UnipolarFloat;
use tunnels::clock_server::StaticClockBank;

use crate::fixture::prelude::*;
use crate::osc::ScopedControlEmitter;
use crate::strobe::{Distributor, StrobeClock};

#[derive(Default)]
pub struct MasterControls {
    strobe_clock: StrobeClock,
    pub clock_state: StaticClockBank,
    pub audio_envelope: UnipolarFloat,
}

impl MasterControls {
    pub fn update(&mut self, delta_t: Duration, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        self.strobe_clock
            .update(delta_t, self.audio_envelope, emitter);
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
