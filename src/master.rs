//! Show-level controls.

use std::time::Duration;

use number::UnipolarFloat;
use tunnels::clock_server::StaticClockBank;

use crate::fixture::prelude::*;
use crate::osc::ScopedControlEmitter;
use crate::strobe::{StrobeClock, StrobeState};

pub struct MasterControls {
    strobe_clock: StrobeClock,
    pub strobe_state: StrobeState,
    pub clock_state: StaticClockBank,
    pub audio_envelope: UnipolarFloat,
}

impl MasterControls {
    pub fn new() -> Self {
        Self {
            strobe_clock: Default::default(),
            strobe_state: Default::default(),
            clock_state: Default::default(),
            audio_envelope: Default::default(),
        }
    }

    pub fn update(&mut self, delta_t: Duration, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        self.strobe_clock
            .update(delta_t, self.audio_envelope, emitter);
        self.strobe_state = self.strobe_clock.state();
    }

    pub fn emit_state(&self, emitter: &dyn EmitControlMessage) {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };
        self.strobe_clock.emit_state(emitter);
    }

    pub fn control(
        &mut self,
        msg: &ControlMessage,
        emitter: &dyn EmitControlMessage,
    ) -> anyhow::Result<()> {
        let emitter = &ScopedControlEmitter {
            entity: GROUP,
            emitter,
        };

        match msg {
            ControlMessage::Strobe(sc) => {
                self.strobe_clock.control(sc, emitter);
            }
        }

        Ok(())
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
}

#[derive(Debug, Clone)]
pub enum ControlMessage {
    #[expect(unused)]
    Strobe(crate::strobe::ControlMessage),
}

#[derive(Debug, Clone)]
pub enum StateChange {
    #[expect(unused)]
    Strobe(crate::strobe::StateChange),
}

pub const GROUP: &str = "Master";
