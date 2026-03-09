//! A control for a string-labeled menu of choices.
//! This models simple things like color selection, where a choice directly corresponds
//! to a DMX value.

use anyhow::bail;
use itertools::Itertools;
use rosc::OscType;

use crate::osc::ScopedOscMessage;

use super::{Bool, OscControl, RenderToDmxWithAnimations};

/// Select from a menu of labeled options.
#[derive(Debug)]
pub struct LabeledSelect {
    /// Currently-selected value.
    selected: usize,
    /// The menu of pairs of label and DMX value.
    options: Vec<(&'static str, u8)>,
    /// Name of this control.
    name: String,

    /// Optional "split color"-style control.
    split: Option<Split>,

    /// Offset into DMX buffer to render into.
    dmx_buf_offset: usize,
}

impl LabeledSelect {
    pub fn new<S: Into<String>>(
        name: S,
        dmx_buf_offset: usize,
        options: Vec<(&'static str, u8)>,
    ) -> Self {
        assert!(!options.is_empty());
        Self {
            selected: 0,
            options,
            name: name.into(),
            split: None,
            dmx_buf_offset,
        }
    }

    /// Add "split color"-style offset. The name of the split button for eg Color
    /// will be SplitColor.
    pub fn with_split(mut self, offset: u8) -> Self {
        self.split = Some(Split {
            split_on: Bool::new_off(format!("Split{}", self.name), ()),
            offset,
        });
        self
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.options.iter().map(|(l, _)| *l)
    }

    /// Return the currently-selected DMX value.
    pub fn dmx_val(&self) -> u8 {
        let mut val = self.options[self.selected].1;
        if let Some(split) = &self.split
            && split.split_on.val()
        {
            val += split.offset;
        }
        val
    }
}

impl OscControl<&str> for LabeledSelect {
    fn control_direct(
        &mut self,
        val: &str,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        let Some(i) = self
            .labels()
            .enumerate()
            .filter_map(|(i, label)| (label == val).then_some(i))
            .next()
        else {
            bail!(
                "the label {val} did not match any valid option for {}:\n{}",
                self.name,
                self.labels().join(", ")
            );
        };
        // If selected is same as current, do nothing.
        if i == self.selected {
            return Ok(());
        }
        self.selected = i;
        self.emit_state(emitter);
        Ok(())
    }

    fn control(
        &mut self,
        msg: &crate::osc::OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if let Some(split) = &mut self.split
            && split.split_on.control(msg, emitter)?
        {
            return Ok(true);
        }
        if msg.control() != self.name {
            return Ok(false);
        }

        // Ignore button release messages.
        if msg.arg == OscType::Float(0.0) {
            return Ok(true);
        }

        let name = msg
            .addr_payload()
            .split('/')
            .nth(1)
            .ok_or_else(|| msg.err("command is missing variant specifier"))?;

        self.control_direct(name, emitter)?;
        Ok(true)
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        if let Some(split) = &self.split {
            split.split_on.emit_state(emitter);
        }
        for (i, label) in self.labels().enumerate() {
            // TODO: consider caching outgoing addresses
            // We could also do this for matching incoming addresses.
            emitter.emit_osc(ScopedOscMessage {
                control: &format!("{}/{}", self.name, label),
                arg: OscType::Float(if i == self.selected { 1.0 } else { 0.0 }),
            });
        }
    }
}

impl super::DescribeOscControls for LabeledSelect {
    fn describe_controls(&self) -> Vec<super::OscControlDescription> {
        let mut controls = vec![super::OscControlDescription {
            name: self.name.clone(),
            control_type: super::OscControlType::LabeledSelect {
                labels: self.options.iter().map(|(l, _)| *l).collect(),
            },
        }];
        if let Some(split) = &self.split {
            controls.extend(split.split_on.describe_controls());
        }
        controls
    }
}

impl RenderToDmxWithAnimations for LabeledSelect {
    fn render(
        &self,
        _group_controls: &crate::fixture::FixtureGroupControls,
        _animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        dmx_buf[self.dmx_buf_offset] = self.dmx_val();
    }
}

/// Configure "split" feature for a LabeledSelect.
#[derive(Debug)]
struct Split {
    split_on: Bool<()>,
    /// The offset to add to the rendered DMX value.
    offset: u8,
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

    fn test_select() -> LabeledSelect {
        LabeledSelect::new("Color", 0, vec![("Red", 10), ("Green", 20), ("Blue", 30)])
    }

    #[test]
    fn test_new_selects_first() {
        let sel = test_select();
        assert_eq!(sel.dmx_val(), 10);
    }

    #[test]
    fn test_control_direct_valid_label() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        sel.control_direct("Green", &emitter).unwrap();
        assert_eq!(sel.dmx_val(), 20);
        let msgs = emitter.take();
        // Should emit radio pattern for all 3 labels
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_control_direct_invalid_errors() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        assert!(sel.control_direct("NoSuch", &emitter).is_err());
    }

    #[test]
    fn test_control_direct_same_value_noop() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        // Already at index 0 ("Red"), selecting "Red" again should be a noop
        sel.control_direct("Red", &emitter).unwrap();
        let msgs = emitter.take();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_control_ignores_release() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Color/Red", OscType::Float(0.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(handled);
        // No state emission on release
        let msgs = emitter.take();
        assert!(msgs.is_empty());
    }

    #[test]
    fn test_control_parses_label() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Color/Red", OscType::Float(1.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(handled);
        // Red is already selected (index 0), so no state change
        // Now select Green
        let msg = make_msg("/g/Color/Green", OscType::Float(1.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert_eq!(sel.dmx_val(), 20);
    }

    #[test]
    fn test_control_non_matching() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Other", OscType::Float(1.0));
        let handled = sel.control(&msg, &emitter).unwrap();
        assert!(!handled);
    }

    #[test]
    fn test_emit_state_radio_pattern() {
        let mut sel = test_select();
        let emitter = MockEmitter::new();
        sel.control_direct("Green", &emitter).unwrap();
        emitter.take(); // clear
        sel.emit_state(&emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 3);
        // Red=0.0, Green=1.0, Blue=0.0
        assert_eq!(msgs[0].1, OscType::Float(0.0));
        assert_eq!(msgs[1].1, OscType::Float(1.0));
        assert_eq!(msgs[2].1, OscType::Float(0.0));
    }

    #[test]
    fn test_dmx_val_with_split() {
        let mut sel = test_select().with_split(5);
        let emitter = MockEmitter::new();
        // Split off by default
        assert_eq!(sel.dmx_val(), 10);
        // Turn on split
        let msg = make_msg("/g/SplitColor", OscType::Float(1.0));
        sel.control(&msg, &emitter).unwrap();
        assert_eq!(sel.dmx_val(), 15); // 10 + 5
    }
}
