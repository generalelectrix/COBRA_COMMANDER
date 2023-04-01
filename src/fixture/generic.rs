//! Control abstractions that are re-usable across fixture types.

use std::time::Duration;

use number::UnipolarFloat;
use simple_error::bail;

use crate::master::Strobe as MasterStrobe;
use crate::{config::Options, util::unipolar_to_range};

/// Most basic strobe control - active/not, plus rate.
#[derive(Default, Clone, Debug)]
pub struct GenericStrobe {
    on: bool,
    rate: UnipolarFloat,
}

impl GenericStrobe {
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn rate(&self) -> UnipolarFloat {
        self.rate
    }

    pub fn emit_state<F>(&self, emit: &mut F)
    where
        F: FnMut(GenericStrobeStateChange),
    {
        use GenericStrobeStateChange::*;
        emit(On(self.on));
        emit(Rate(self.rate));
    }

    pub fn handle_state_change(&mut self, sc: GenericStrobeStateChange) {
        use GenericStrobeStateChange::*;
        match sc {
            On(v) => self.on = v,
            Rate(v) => self.rate = v,
        }
    }

    /// Render as a single DMX range with off.
    #[allow(dead_code)]
    pub fn render_range(&self, off: u8, slow: u8, fast: u8) -> u8 {
        if self.on {
            unipolar_to_range(slow, fast, self.rate)
        } else {
            off
        }
    }

    /// Render as a single DMX range with off, using master as an override.
    /// Only strobe if master strobe is on and the local strobe is also on.
    /// Always use the master strobe rate.
    pub fn render_range_with_master(
        &self,
        master: &MasterStrobe,
        off: u8,
        slow: u8,
        fast: u8,
    ) -> u8 {
        let rate = if master.use_master_rate {
            master.state.rate
        } else {
            self.rate
        };
        if self.on && master.state.on {
            unipolar_to_range(slow, fast, rate)
        } else {
            off
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GenericStrobeStateChange {
    On(bool),
    Rate(UnipolarFloat),
}

#[derive(Debug)]
pub struct Timer {
    /// Total time in a full timer cycle (timer period).
    duration: Duration,
    /// The fraction of the cycle that the timer is "on".
    duty_cycle: UnipolarFloat,
    is_on: bool,
    state_age: Duration,
}

fn parse_seconds(options: &Options, key: &str) -> Result<Option<Duration>, String> {
    let Some(v) = options
        .get(key)
        else { return Ok(None)};
    let secs = v
        .parse::<u64>()
        .map_err(|e| format!("{}: expected integer seconds: {}", key, e))?;
    Ok(Some(Duration::from_secs(secs)))
}

impl Timer {
    /// The amount of time the timer stays on during a cycle.
    fn on_duration(&self) -> Duration {
        self.duration.mul_f64(self.duty_cycle.val())
    }

    /// The amount of time the timer stays off during a cycle.
    fn off_duration(&self) -> Duration {
        self.duration.mul_f64(1.0 - self.duty_cycle.val())
    }

    pub fn from_options(options: &Options) -> Result<Self, String> {
        match (
            parse_seconds(options, "timer_on")?,
            parse_seconds(options, "timer_off")?,
        ) {
            (Some(on), Some(off)) => {
                let duration = on + off;
                let duty_cycle = UnipolarFloat::new(on.as_secs_f64() / duration.as_secs_f64());
                Ok(Self::new(duration, duty_cycle))
            }
            (None, None) => Ok(Self::new(Duration::from_secs(360), UnipolarFloat::new(0.5))),
            _ => Err(format!("bad timer options: {:?}", options)),
        }
    }

    pub fn new(duration: Duration, duty_cycle: UnipolarFloat) -> Self {
        Self {
            duration,
            duty_cycle,
            is_on: true,
            state_age: Duration::ZERO,
        }
    }

    pub fn update(&mut self, delta_t: Duration) {
        let new_state_age = self.state_age + delta_t;
        let dwell = if self.is_on {
            self.on_duration()
        } else {
            self.off_duration()
        };
        if new_state_age >= dwell {
            self.is_on = !self.is_on;
            self.state_age = Duration::ZERO;
        } else {
            self.state_age = new_state_age;
        }
    }

    pub fn is_on(&self) -> bool {
        self.is_on
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.is_on = true;
        self.state_age = Duration::ZERO;
    }

    pub fn emit_state<F>(&self, emit: &mut F)
    where
        F: FnMut(TimerStateChange),
    {
        use TimerStateChange::*;
        emit(Duration(self.duration));
        emit(DutyCycle(self.duty_cycle));
    }

    pub fn handle_state_change(&mut self, sc: TimerStateChange) {
        use TimerStateChange::*;
        match sc {
            Duration(d) => self.duration = d,
            DutyCycle(d) => self.duty_cycle = d,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TimerStateChange {
    Duration(Duration),
    DutyCycle(UnipolarFloat),
}
