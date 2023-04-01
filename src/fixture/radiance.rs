//! Control profile for a Radiance hazer.
//! Probably fine for any generic 2-channel hazer.

use std::{collections::HashMap, error::Error, time::Duration};

use log::error;
use number::UnipolarFloat;

use super::{
    generic::{Timer, TimerStateChange},
    EmitFixtureStateChange, Fixture, FixtureControlMessage, PatchFixture,
};
use crate::{master::MasterControls, util::unipolar_to_range};

#[derive(Default, Debug)]
pub struct Radiance {
    haze: UnipolarFloat,
    fan: UnipolarFloat,
    timer: Option<Timer>,
}

impl PatchFixture for Radiance {
    const NAME: &'static str = "radiance";
    fn channel_count(&self) -> usize {
        2
    }

    fn new(options: &HashMap<String, String>) -> Result<Self, Box<dyn Error>> {
        let mut s = Self::default();
        if options.contains_key("use_timer") {
            s.timer = Some(Timer::from_options(options)?);
        }
        Ok(s)
    }
}

impl Radiance {
    fn handle_state_change(&mut self, sc: StateChange, emitter: &mut dyn EmitFixtureStateChange) {
        use StateChange::*;
        match sc {
            Haze(v) => self.haze = v,
            Fan(v) => self.fan = v,
            Timer(t) => {
                if let Some(ref mut timer) = self.timer {
                    timer.handle_state_change(t);
                } else {
                    error!("radiance got a timer state change but has no timer");
                }
            }
        };
        emitter.emit_radiance(sc);
    }
}

impl Fixture for Radiance {
    fn update(&mut self, delta_t: Duration) {
        if let Some(timer) = self.timer.as_mut() {
            timer.update(delta_t);
        }
    }
    fn render(&self, _master_controls: &MasterControls, dmx_buf: &mut [u8]) {
        if let Some(timer) = self.timer.as_ref() {
            if !timer.is_on() {
                dmx_buf[0] = 0;
                dmx_buf[1] = 0;
                return;
            }
        }
        dmx_buf[0] = unipolar_to_range(0, 255, self.haze * UnipolarFloat::new(0.2));
        dmx_buf[1] = unipolar_to_range(0, 255, self.fan);
    }

    fn emit_state(&self, emitter: &mut dyn EmitFixtureStateChange) {
        use StateChange::*;
        emitter.emit_radiance(Haze(self.haze));
        emitter.emit_radiance(Fan(self.fan));
        let mut emit_timer = |ssc| {
            emitter.emit_radiance(Timer(ssc));
        };
        if let Some(ref timer) = self.timer {
            timer.emit_state(&mut emit_timer);
        }
    }

    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitFixtureStateChange,
    ) -> Option<FixtureControlMessage> {
        match msg {
            FixtureControlMessage::Radiance(msg) => {
                self.handle_state_change(msg, emitter);
                None
            }
            other => Some(other),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    Haze(UnipolarFloat),
    Fan(UnipolarFloat),
    Timer(TimerStateChange),
}

// Venus has no controls that are not represented as state changes.
pub type ControlMessage = StateChange;
