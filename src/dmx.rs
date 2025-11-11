use std::{fmt::Display, ops::Add};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// A DMX address, indexed from 1.
///
/// We don't check that the value is valid at parse time, as this makes
/// deserializing into an untagged ParseDmxAddr fail with an obscure message.
/// This needs to be validated downstream.
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Copy, Clone, Debug)]
pub struct DmxAddr(usize);

impl DmxAddr {
    /// Get the DMX buffer index of this address (indexed from 0).
    pub fn dmx_index(&self) -> usize {
        self.0 - 1
    }

    /// Ensure this address is in range.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=512).contains(&self.0),
            "invalid DMX address {}",
            self.0
        );
        Ok(())
    }
}

impl Display for DmxAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<usize> for DmxAddr {
    type Output = DmxAddr;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

/// A data buffer for one DMX universe.
pub type DmxBuffer = [u8; 512];

/// Index into the DMX universes.
pub type UniverseIdx = usize;
