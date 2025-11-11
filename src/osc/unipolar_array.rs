//! TouchOSC array of unipolar controls, such as upfaders.
use number::UnipolarFloat;
use rosc::OscType;

use super::{GroupControlMap, ScopedOscMessage};
use anyhow::{Result, bail};

use anyhow::{Context, anyhow};

/// Model a unipolar control array.
#[derive(Clone)]
pub struct UnipolarArray {
    pub control: &'static str,
}

/// Create a unipolar control array.
pub const fn unipolar_array(control: &'static str) -> UnipolarArray {
    UnipolarArray { control }
}

impl UnipolarArray {
    /// Wire up this unipolar control array to a control map.
    pub fn map<F, T>(self, map: &mut GroupControlMap<T>, process: F)
    where
        F: Fn(usize, UnipolarFloat) -> Result<T> + 'static + Copy,
    {
        map.add(self.control, move |msg| {
            let index = msg
                .addr_payload()
                .split('/')
                .skip(1)
                .take(1)
                .next()
                .ok_or_else(|| anyhow!("unipolar control array index missing for {msg:?}"))?
                .parse::<usize>()
                .with_context(|| format!("handling message {msg:?}"))?;
            if index == 0 {
                bail!("unipolar control array index is 0: {msg:?}");
            }
            let val = msg.get_unipolar()?;
            process(index - 1, val).map(Some)
        })
    }

    /// Emit state for a particular unipolar control index.
    pub fn set<S>(&self, n: usize, val: UnipolarFloat, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        emitter.emit_osc(ScopedOscMessage {
            control: &format!("{}/{}", self.control, n + 1),
            arg: OscType::Float(val.val() as f32),
        });
    }
}
