//! Control abstractions that are re-usable across fixture types.
use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::config::Options;

#[derive(Debug)]
pub struct Timer {
    pub on: Duration,
    pub off: Duration,
    is_on: bool,
    state_age: Duration,
}

fn parse_seconds(options: &Options, key: &str) -> Result<Duration> {
    let v = options
        .get(key)
        .ok_or_else(|| anyhow!("missing options key \"{}\"", key))?;
    let secs = v
        .parse::<u64>()
        .map_err(|e| anyhow!("{}: expected integer seconds: {}", key, e))?;
    Ok(Duration::from_secs(secs))
}

impl Timer {
    pub fn from_options(options: &Options) -> Result<Self> {
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

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.is_on = true;
        self.state_age = Duration::ZERO;
    }
}
