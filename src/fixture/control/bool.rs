//! A control for boolean values.

use anyhow::Context;

use crate::osc::{EmitScopedOscMessage, OscControlMessage};

use super::{OscControl, RenderToDmx, RenderToDmxWithAnimations};

/// A bool value, with controls.
#[derive(Debug)]
pub struct Bool<R: RenderToDmx<bool>> {
    val: bool,
    name: String,
    render: R,
}

/// A bool control that renders into a single DMX channel at full range.
pub type BoolChannel = Bool<RenderBoolToRange>;

impl<R: RenderToDmx<bool>> Bool<R> {
    /// Initialize a new control with the provided OSC control name.
    pub fn new<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: false,
            name: name.into(),
            render,
        }
    }
}

impl Bool<RenderBoolToRange> {
    /// Initialize a bool control that renders to DMX 0/255.
    pub fn full_channel<S: Into<String>>(name: S, dmx_buf_offset: usize) -> Self {
        Self::channel(name, dmx_buf_offset, 0, 255)
    }

    /// Initialize a bool control that renders to DMX vals for off/on.
    pub fn channel<S: Into<String>>(name: S, dmx_buf_offset: usize, off: u8, on: u8) -> Self {
        Self::new(
            name,
            RenderBoolToRange {
                dmx_buf_offset,
                off,
                on,
            },
        )
    }
}

impl<R: RenderToDmx<bool>> OscControl<bool> for Bool<R> {
    fn val(&self) -> &bool {
        &self.val
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if msg.control() != self.name {
            return Ok(false);
        }
        let v = msg.get_bool().with_context(|| self.name.clone())?;
        self.val = v;
        emitter.emit_float(&self.name, self.val.into());
        Ok(true)
    }

    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage) {
        emitter.emit_float(&self.name, self.val.into());
    }
}

impl<R: RenderToDmx<bool>> RenderToDmxWithAnimations for Bool<R> {
    fn render(&self, _animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        self.render.render(&self.val, dmx_buf);
    }
}

/// Render a bool float to fixed values.
#[derive(Debug)]
pub struct RenderBoolToRange {
    pub dmx_buf_offset: usize,
    pub off: u8,
    pub on: u8,
}

impl RenderToDmx<bool> for RenderBoolToRange {
    fn render(&self, val: &bool, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = if *val { self.on } else { self.off }
    }
}
