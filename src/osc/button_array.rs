//! An array of buttons that adhere to an expected address structure.
//!
//! This need not be an TouchOSC "button grid".
//! Note that the OSC controls should be indexed starting at 1, not 0, and these
//! indexes will be shifted into a 0-indexed space when handled.
use rosc::OscType;

use crate::osc::OscControlMessage;

use super::{GroupControlMap, ScopedOscMessage};
use anyhow::{bail, Result};

use anyhow::{anyhow, Context};

/// Model a button array.
#[derive(Clone)]
pub struct ButtonArray {
    pub control: &'static str,
}

/// Create a button array.
pub const fn button_array(control: &'static str) -> ButtonArray {
    ButtonArray { control }
}

impl ButtonArray {
    /// Wire up this button array to a control map.
    pub fn map<F, T>(self, map: &mut GroupControlMap<T>, process: F)
    where
        F: Fn(usize) -> T + 'static + Copy,
    {
        map.add(self.control, move |msg| {
            self.parse(msg)?.map(|i| Ok(process(i))).transpose()
        })
    }

    /// Get a index from a button array.
    fn parse(&self, msg: &OscControlMessage) -> Result<Option<usize>> {
        let index = msg
            .addr_payload()
            .split('/')
            .skip(1)
            .take(1)
            .next()
            .ok_or_else(|| anyhow!("button array index missing for {msg:?}"))?
            .parse::<usize>()
            .with_context(|| format!("handling message {msg:?}"))?;
        if index == 0 {
            bail!("button array index is 0: {msg:?}");
        }
        // Ignore button release messages.
        if msg.arg == OscType::Float(0.0) {
            return Ok(None);
        }
        Ok(Some(index - 1))
    }

    /// Emit state for a particular button index.
    pub fn set<S>(&self, n: usize, val: bool, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        emitter.emit_osc(ScopedOscMessage {
            control: &format!("{}/{}", self.control, n + 1),
            arg: OscType::Float(if val { 1.0 } else { 0.0 }),
        });
    }
}
