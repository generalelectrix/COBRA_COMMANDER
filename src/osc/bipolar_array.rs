//! TouchOSC array of bipolar controls, such as upfaders.
use number::BipolarFloat;
use rosc::OscType;

use super::{GroupControlMap, ScopedOscMessage};
use anyhow::{Result, bail};

use anyhow::{Context, anyhow};

/// Model a bipolar control array.
#[derive(Clone)]
pub struct BipolarArray {
    pub control: &'static str,
}

/// Create a bipolar control array.
pub const fn bipolar_array(control: &'static str) -> BipolarArray {
    BipolarArray { control }
}

impl BipolarArray {
    /// Wire up this bipolar control array to a control map.
    pub fn map<F, T>(self, map: &mut GroupControlMap<T>, process: F)
    where
        F: Fn(usize, BipolarFloat) -> Result<T> + 'static + Copy,
    {
        map.add(self.control, move |msg| {
            let index = msg
                .addr_payload()
                .split('/')
                .skip(1)
                .take(1)
                .next()
                .ok_or_else(|| anyhow!("bipolar control array index missing for {msg:?}"))?
                .parse::<usize>()
                .with_context(|| format!("handling message {msg:?}"))?;
            if index == 0 {
                bail!("bipolar control array index is 0: {msg:?}");
            }
            let val = msg.get_bipolar()?;
            process(index - 1, val).map(Some)
        })
    }

    /// Emit state for a particular bipolar control index.
    pub fn set<S>(&self, n: usize, val: BipolarFloat, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        emitter.emit_osc(ScopedOscMessage {
            control: &format!("{}/{}", self.control, n + 1),
            arg: OscType::Float(val.val() as f32),
        });
    }
}
