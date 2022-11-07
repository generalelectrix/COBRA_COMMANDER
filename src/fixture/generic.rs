//! Control abstractions that are re-usable across fixture types.

use std::time::Duration;

use number::UnipolarFloat;

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
    pub fn render_range_with_master(&self, master: &Self, off: u8, slow: u8, fast: u8) -> u8 {
        if self.on && master.on {
            unipolar_to_range(slow, fast, master.rate)
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
    pub on: Duration,
    pub off: Duration,
    is_on: bool,
    state_age: Duration,
}

fn parse_seconds(options: &Options, key: &str) -> Result<Duration, String> {
    let v = options
        .get(key)
        .ok_or_else(|| format!("missing options key \"{}\"", key))?;
    let secs = u64::from_str_radix(v, 10)
        .map_err(|e| format!("{}: expected integer seconds: {}", key, e))?;
    Ok(Duration::from_secs(secs))
}

impl Timer {
    pub fn from_options(options: &Options) -> Result<Self, String> {
        let on = parse_seconds(options, "timer_on")?;
        let off = parse_seconds(options, "timer_off")?;
        Ok(Self::new(on, off))
    }

    pub fn new(on: Duration, off: Duration) -> Self {
        Self {
            on,
            off,
            is_on: true,
            state_age: Duration::ZERO,
        }
    }

    pub fn update(&mut self, delta_t: Duration) {
        let new_state_age = self.state_age + delta_t;
        let dwell = if self.is_on { self.on } else { self.off };
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

    pub fn reset(&mut self) {
        self.is_on = true;
        self.state_age = Duration::ZERO;
    }
}
