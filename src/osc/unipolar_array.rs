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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{MockEmitter, OscClientId};
    use rosc::OscMessage;

    #[derive(Debug, PartialEq)]
    enum Msg {
        Value(usize, UnipolarFloat),
    }

    fn make_msg(addr: &str, arg: OscType) -> crate::osc::OscControlMessage {
        crate::osc::OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    #[test]
    fn test_valid_index_and_value() {
        let ua = unipolar_array("Ctrl");
        let mut map = GroupControlMap::default();
        ua.map(&mut map, |i, v| Ok(Msg::Value(i, v)));
        let msg = make_msg("/group/Ctrl/3", OscType::Float(0.5));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::Value(2, UnipolarFloat::new(0.5)));
    }

    #[test]
    fn test_zero_index_errors() {
        let ua = unipolar_array("Ctrl");
        let mut map = GroupControlMap::default();
        ua.map(&mut map, |i, v| Ok(Msg::Value(i, v)));
        let msg = make_msg("/group/Ctrl/0", OscType::Float(0.5));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_missing_index_errors() {
        let ua = unipolar_array("Ctrl");
        let mut map = GroupControlMap::default();
        ua.map(&mut map, |i, v| Ok(Msg::Value(i, v)));
        let msg = make_msg("/group/Ctrl", OscType::Float(0.5));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_set_emits_correct_addr() {
        let ua = unipolar_array("Ctrl");
        let emitter = MockEmitter::new();
        ua.set(2, UnipolarFloat::new(0.5), &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], ("Ctrl/3".to_string(), OscType::Float(0.5)));
    }
}
