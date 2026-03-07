//! A radio button-style control that selects from a continuous integer range.

use anyhow::{Result, anyhow, ensure};
use rosc::OscType;

use crate::osc::{EmitScopedOscMessage, OscControlMessage, ScopedOscMessage};

use super::{OscControl, RenderToDmx, RenderToDmxWithAnimations};

/// A control for selecting a numeric index.
/// Model a 1D button grid with radio-select behavior.
/// This implements the TouchOSC model for a button grid.
/// Special-cased to handle only 1D grids.
#[derive(Debug)]
pub struct IndexedSelect<R: RenderToDmx<usize>> {
    /// Currently-selected value.
    val: usize,
    /// The largest index.
    n: usize,
    name: String,
    /// If true, use the 0th coordinate as the index.  If false, use the 1st coordinate.
    /// FIXME: this forces us to encode the orientation of the TouchOSC layout into
    /// the control profile.  We might want to replace the button grids with individual
    /// buttons in the future to fix this.
    pub x_primary_coordinate: bool,
    render: R,
}

pub type IndexedSelectMenu = IndexedSelect<RenderIndexedSelectToFixedValues>;
pub type IndexedSelectMult = IndexedSelect<RenderIndexedSelectToMultiple>;

impl<R: RenderToDmx<usize>> IndexedSelect<R> {
    /// Initialize a new control with the provided OSC control name.
    pub fn new<S: Into<String>>(name: S, n: usize, x_primary_coordinate: bool, render: R) -> Self {
        Self {
            val: 0,
            n,
            name: name.into(),
            x_primary_coordinate,
            render,
        }
    }

    pub fn selected(&self) -> usize {
        self.val
    }
}

impl IndexedSelect<RenderIndexedSelectToFixedValues> {
    pub fn fixed_values<S: Into<String>>(
        name: S,
        dmx_buf_offset: usize,
        x_primary_coordinate: bool,
        vals: &'static [u8],
    ) -> Self {
        Self::new(
            name,
            vals.len(),
            x_primary_coordinate,
            RenderIndexedSelectToFixedValues {
                dmx_buf_offset,
                vals,
            },
        )
    }
}

impl IndexedSelect<RenderIndexedSelectToMultiple> {
    /// An IndexedSelect rendered to DMX using a fixed multiple of the index.
    /// Also adds the provided fixed offset to all values.
    pub fn multiple<S: Into<String>>(
        name: S,
        dmx_buf_offset: usize,
        x_primary_coordinate: bool,
        n: usize,
        mult: usize,
        offset: usize,
    ) -> Self {
        assert!(n > 0);
        assert!((n - 1) * mult + offset <= u8::MAX as usize);
        Self::new(
            name,
            n,
            x_primary_coordinate,
            RenderIndexedSelectToMultiple {
                dmx_buf_offset,
                mult,
                offset,
            },
        )
    }
}

impl<R: RenderToDmx<usize>> OscControl<usize> for IndexedSelect<R> {
    fn control_direct(
        &mut self,
        val: usize,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        ensure!(
            val < self.n,
            "direct control value {val} for {} is out of range (max {})",
            self.name,
            self.n - 1
        );
        // No action needed if we pressed the select for the current value.
        if val == self.val {
            return Ok(());
        }

        self.val = val;
        self.emit_state(emitter);
        Ok(())
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if msg.control() != self.name {
            return Ok(false);
        }

        // Ignore button release messages.
        if msg.arg == OscType::Float(0.0) {
            return Ok(true);
        }

        let (x, y) = parse_radio_button_indices(msg.addr_payload())?;
        let (primary, secondary) = if self.x_primary_coordinate {
            (x, y)
        } else {
            (y, x)
        };
        ensure!(
            primary < self.n,
            "primary index for {} out of range: {primary}",
            self.name
        );
        ensure!(
            secondary == 0,
            "secondary index for {} unexpectedly non-zero: {secondary}",
            self.name
        );

        self.control_direct(primary, emitter)?;
        Ok(true)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&usize),
    ) -> anyhow::Result<bool> {
        if self.control(msg, emitter)? {
            callback(&self.val);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage) {
        for i in 0..self.n {
            let val = if i == self.val { 1.0 } else { 0.0 };
            let (x, y) = if self.x_primary_coordinate {
                (i + 1, 1)
            } else {
                (1, i + 1)
            };
            emitter.emit_osc(ScopedOscMessage {
                control: &format!("{}/{}/{}", self.name, x, y),
                arg: OscType::Float(val),
            })
        }
    }

    fn emit_state_with_callback(
        &self,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&usize),
    ) {
        self.emit_state(emitter);
        callback(&self.val);
    }
}

impl<R: RenderToDmx<usize>> super::DescribeOscControls for IndexedSelect<R> {
    fn describe_controls(&self) -> Vec<super::OscControlDescription> {
        vec![super::OscControlDescription {
            name: self.name.clone(),
            control_type: super::OscControlType::IndexedSelect {
                n: self.n,
                x_primary_coordinate: self.x_primary_coordinate,
            },
        }]
    }
}

impl<R: RenderToDmx<usize>> RenderToDmxWithAnimations for IndexedSelect<R> {
    fn render(
        &self,
        _group_controls: &crate::fixture::FixtureGroupControls,
        _animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        self.render.render(&self.val, dmx_buf);
    }
}

/// Render a indexed select float to a fixed collection of values.
#[derive(Debug)]
pub struct RenderIndexedSelectToFixedValues {
    pub dmx_buf_offset: usize,
    pub vals: &'static [u8],
}

impl RenderToDmx<usize> for RenderIndexedSelectToFixedValues {
    fn render(&self, val: &usize, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = self.vals[*val];
    }
}

/// Render a indexed select float to a multiple of the index.
#[derive(Debug)]
pub struct RenderIndexedSelectToMultiple {
    pub dmx_buf_offset: usize,
    pub mult: usize,
    pub offset: usize,
}

impl RenderToDmx<usize> for RenderIndexedSelectToMultiple {
    fn render(&self, val: &usize, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = (*val * self.mult + self.offset) as u8;
    }
}

/// Parse radio button indices from a TouchOSC button grid.
fn parse_radio_button_indices(addr_payload: &str) -> Result<(usize, usize)> {
    let mut pieces_iter = addr_payload
        .split('/')
        .skip(1)
        .take(2)
        .map(str::parse::<usize>);
    let x = pieces_iter
        .next()
        .ok_or_else(|| anyhow!("x radio button index missing"))?
        .map_err(|err| anyhow!("failed to parse radio button x index: {}", err))?;
    let y = pieces_iter
        .next()
        .ok_or_else(|| anyhow!("y radio button index missing"))?
        .map_err(|err| anyhow!("failed to parse radio button y index: {}", err))?;
    ensure!(x != 0, "x index is unexpectedly 0");
    ensure!(y != 0, "y index is unexpectedly 0");
    Ok((x - 1, y - 1))
}

#[cfg(test)]
mod tests {
    use rosc::{OscMessage, OscType};

    use crate::osc::{MockEmitter, OscClientId, OscControlMessage};

    use super::*;

    fn make_msg(addr: &str, arg: OscType) -> OscControlMessage {
        OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    fn test_select() -> IndexedSelect<RenderIndexedSelectToFixedValues> {
        IndexedSelect::fixed_values("Ctrl", 0, true, &[10, 20, 30])
    }

    #[test]
    fn test_new_starts_at_zero() {
        let sel = test_select();
        assert_eq!(sel.selected(), 0);
    }

    #[test]
    fn test_control_direct_valid() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        sel.control_direct(2, &emitter).unwrap();
        assert_eq!(sel.selected(), 2);
        let msgs = emitter.take();
        // Should emit radio pattern: n=3 messages
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_control_direct_out_of_range() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        assert!(sel.control_direct(5, &emitter).is_err());
    }

    #[test]
    fn test_control_direct_same_noop() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        // Already at 0, selecting 0 again should be silent
        sel.control_direct(0, &emitter).unwrap();
        let msgs = emitter.take();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_control_ignores_release() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        // x_primary_coordinate=true, so index comes from first coord
        let msg = make_msg("/g/Ctrl/2/1", OscType::Float(0.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(handled);
        // No state change on release
        assert_eq!(sel.selected(), 0);
    }

    #[test]
    fn test_control_parses_radio_indices() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        // x_primary_coordinate=true: /2/1 means x=2,y=1 → primary=x-1=1, secondary=y-1=0
        let msg = make_msg("/g/Ctrl/2/1", OscType::Float(1.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert_eq!(sel.selected(), 1);
    }

    #[test]
    fn test_control_non_matching() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Other/1/1", OscType::Float(1.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(!handled);
    }

    #[test]
    fn test_emit_state_radio_pattern() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        sel.control_direct(1, &emitter).unwrap();
        emitter.take(); // clear
        sel.emit_state(&emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 3);
        // Index 0: 0.0, Index 1: 1.0, Index 2: 0.0
        assert_eq!(msgs[0].1, OscType::Float(0.0));
        assert_eq!(msgs[1].1, OscType::Float(1.0));
        assert_eq!(msgs[2].1, OscType::Float(0.0));
    }

    #[test]
    fn test_render_fixed_values() {
        let render = RenderIndexedSelectToFixedValues {
            dmx_buf_offset: 0,
            vals: &[10, 20, 30],
        };
        let mut buf = [0u8; 1];
        render.render(&1, &mut buf);
        assert_eq!(buf[0], 20);
    }

    #[test]
    fn test_render_multiple() {
        let render = RenderIndexedSelectToMultiple {
            dmx_buf_offset: 0,
            mult: 10,
            offset: 5,
        };
        let mut buf = [0u8; 1];
        render.render(&2, &mut buf);
        assert_eq!(buf[0], 25); // 2*10 + 5
    }
}
