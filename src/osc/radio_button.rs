use log::error;
use rosc::OscType;

use super::{control_message::OscControlMessage, GroupControlMap, OscError, ScopedOscMessage};
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
    pub fn set<S>(&self, n: usize, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        if n >= self.n {
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
