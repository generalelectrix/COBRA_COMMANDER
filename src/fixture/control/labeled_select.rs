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
        if let Some(split) = &self.split {
            if split.split_on.val() {
                val += split.offset;
            }
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
        if let Some(split) = &mut self.split {
            if split.split_on.control(msg, emitter)? {
                return Ok(true);
            }
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
