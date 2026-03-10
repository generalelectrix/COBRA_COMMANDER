use log::error;
use rosc::OscType;

use super::{GroupControlMap, OscError, ScopedOscMessage, control_message::OscControlMessage};
use anyhow::Result;

/// Model a 1D button grid with radio-select behavior.
/// This implements the TouchOSC model for a button grid.
/// Special-cased to handle only 1D grids.
#[derive(Clone)]
pub struct RadioButton {
    pub control: &'static str,
    pub n: usize,
    /// If true, use the 0th coordinate as the index.  If false, use the 1st coordinate.
    /// FIXME: this forces us to encode the orientation of the TouchOSC layout into
    /// the control profile.  We might want to replace the button grids with individual
    /// buttons in the future to fix this.
    pub x_primary_coordinate: bool,
}

impl RadioButton {
    /// Wire up this radio button to a control map.
    pub fn map<F, T>(self, map: &mut GroupControlMap<T>, process: F)
    where
        F: Fn(usize) -> T + 'static + Copy,
    {
        self.map_fallible(map, move |x| Ok(process(x)));
    }

    /// Wire up this radio button to a control map, with a fallible processor.
    pub fn map_fallible<F, T>(self, map: &mut GroupControlMap<T>, process: F)
    where
        F: Fn(usize) -> Result<T> + 'static + Copy,
    {
        map.add(self.control, move |m| {
            self.parse(m)?.map(process).transpose()
        })
    }

    /// Get a index from a collection of radio buttons, mapped to numeric addresses.
    fn parse(&self, v: &OscControlMessage) -> Result<Option<usize>, OscError> {
        let (x, y) = match parse_radio_button_indices(v.addr_payload()) {
            Ok(indices) => indices,
            Err(err) => {
                return Err(v.err(err));
            }
        };
        let (primary, secondary) = if self.x_primary_coordinate {
            (x, y)
        } else {
            (y, x)
        };
        if primary >= self.n {
            return Err(v.err(format!(
                "radio button primary index out of range: {primary}"
            )));
        }
        if secondary > 0 {
            return Err(v.err(format!(
                "radio button secondary index out of range: {secondary}"
            )));
        }
        // Ignore button release messages.
        if v.arg == OscType::Float(0.0) {
            return Ok(None);
        }
        Ok(Some(primary))
    }

    /// Send OSC messages to set the current state of the button.
    /// Error conditions are logged.
    pub fn set<S>(&self, n: usize, allow_out_of_range: bool, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        if n >= self.n && !allow_out_of_range {
            error!("radio button index {} out of range for {}", n, self.control);
            return;
        }
        for i in 0..self.n {
            let val = if i == n { 1.0 } else { 0.0 };
            let (x, y) = if self.x_primary_coordinate {
                (i + 1, 1)
            } else {
                (1, i + 1)
            };
            emitter.emit_osc(ScopedOscMessage {
                control: &format!("{}/{}/{}", self.control, x, y),
                arg: OscType::Float(val),
            })
        }
    }
}

/// Parse radio button indices from a TouchOSC button grid.
fn parse_radio_button_indices(addr_payload: &str) -> Result<(usize, usize), String> {
    let mut pieces_iter = addr_payload
        .split('/')
        .skip(1)
        .take(2)
        .map(str::parse::<usize>);
    let x = pieces_iter
        .next()
        .ok_or_else(|| "x radio button index missing".to_string())?
        .map_err(|err| format!("failed to parse radio button x index: {err}"))?;
    let y = pieces_iter
        .next()
        .ok_or_else(|| "y radio button index missing".to_string())?
        .map_err(|err| format!("failed to parse radio button y index: {err}"))?;
    if x == 0 {
        return Err("x index is unexpectedly 0".to_string());
    }
    if y == 0 {
        return Err("y index is unexpectedly 0".to_string());
    }
    Ok((x - 1, y - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{MockEmitter, OscClientId};
    use rosc::OscMessage;

    #[derive(Debug, PartialEq)]
    enum Msg {
        Selected(usize),
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

    fn make_radio(x_primary: bool, n: usize) -> RadioButton {
        RadioButton {
            control: "Ctrl",
            n,
            x_primary_coordinate: x_primary,
        }
    }

    #[test]
    fn test_valid_press_x_primary() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        let msg = make_msg("/group/Ctrl/1/1", OscType::Float(1.0));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::Selected(0));
    }

    #[test]
    fn test_ignores_release() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        let msg = make_msg("/group/Ctrl/1/1", OscType::Float(0.0));
        let result = map.handle(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_out_of_range_errors() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        // index 4 (0-based 3) is >= n=3
        let msg = make_msg("/group/Ctrl/4/1", OscType::Float(1.0));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_secondary_nonzero_errors() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        // secondary (y) = 1 (0-based) which is > 0
        let msg = make_msg("/group/Ctrl/1/2", OscType::Float(1.0));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_y_primary_swaps_axes() {
        let rb = make_radio(false, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        // x_primary=false, so y is primary. /1/2 → x=0,y=1 → primary=y=1
        let msg = make_msg("/group/Ctrl/1/2", OscType::Float(1.0));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::Selected(1));
    }

    #[test]
    fn test_zero_index_errors() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        let msg = make_msg("/group/Ctrl/0/1", OscType::Float(1.0));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_missing_indices_errors() {
        let rb = make_radio(true, 3);
        let mut map = GroupControlMap::default();
        rb.map(&mut map, Msg::Selected);
        // No indices after control — addr_payload is empty
        let msg = make_msg("/group/Ctrl", OscType::Float(1.0));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_set_emits_correct_pattern() {
        let rb = make_radio(true, 3);
        let emitter = MockEmitter::new();
        rb.set(1, false, &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0], ("Ctrl/1/1".to_string(), OscType::Float(0.0)));
        assert_eq!(msgs[1], ("Ctrl/2/1".to_string(), OscType::Float(1.0)));
        assert_eq!(msgs[2], ("Ctrl/3/1".to_string(), OscType::Float(0.0)));
    }

    #[test]
    fn test_set_out_of_range_no_emit() {
        let rb = make_radio(true, 3);
        let emitter = MockEmitter::new();
        rb.set(5, false, &emitter);
        let msgs = emitter.take();
        assert!(msgs.is_empty());
    }
}
