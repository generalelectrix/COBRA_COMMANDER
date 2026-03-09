//! A control for boolean values.

use anyhow::Context;

use crate::osc::{EmitScopedOscMessage, OscControlMessage};

use super::{
    ChannelControl, ChannelLevelBool, ChannelLevelHandler, OscControl, RenderToDmx,
    RenderToDmxWithAnimations,
};

/// A bool value, with controls.
#[derive(Debug)]
pub struct Bool<R: RenderToDmx<bool>> {
    val: bool,
    name: String,
    render: R,
    strobed: bool,
}

/// A bool control that renders into a single DMX channel at full range.
pub type BoolChannel = Bool<RenderBoolToRange>;

impl<R: RenderToDmx<bool>> Bool<R> {
    /// Initialize a new control with the provided OSC control name.
    /// The control defaults to being off.
    pub fn new_off<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: false,
            name: name.into(),
            render,
            strobed: false,
        }
    }

    /// Initialize a new control with the provided OSC control name.
    /// The control defaults to being on.
    pub fn new_on<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: true,
            name: name.into(),
            render,
            strobed: false,
        }
    }

    pub fn val(&self) -> bool {
        self.val
    }

    /// Listen to the global strobe clock.
    pub fn strobed(mut self) -> Self {
        self.strobed = true;
        self
    }

    pub fn with_channel_level(self) -> ChannelLevelBool<Self> {
        ChannelControl::wrap(self, "Level".to_string(), true, ChannelLevelHandler)
    }
}

impl Bool<RenderBoolToRange> {
    /// Initialize a bool control that renders to DMX 0/255.
    pub fn full_channel<S: Into<String>>(name: S, dmx_buf_offset: usize) -> Self {
        Self::channel(name, dmx_buf_offset, 0, 255)
    }

    /// Initialize a bool control that renders to DMX vals for off/on.
    pub fn channel<S: Into<String>>(name: S, dmx_buf_offset: usize, off: u8, on: u8) -> Self {
        Self::new_off(
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
    fn control_direct(
        &mut self,
        val: bool,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        self.val = val;
        emitter.emit_float(&self.name, self.val.into());
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
        self.control_direct(msg.get_bool().with_context(|| self.name.clone())?, emitter)?;
        Ok(true)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&bool),
    ) -> anyhow::Result<bool> {
        if self.control(msg, emitter)? {
            callback(&self.val);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage) {
        emitter.emit_float(&self.name, self.val.into());
    }

    fn emit_state_with_callback(
        &self,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&bool),
    ) {
        self.emit_state(emitter);
        callback(&self.val);
    }
}

impl<R: RenderToDmx<bool>> super::DescribeOscControls for Bool<R> {
    fn describe_controls(&self) -> Vec<super::OscControlDescription> {
        vec![super::OscControlDescription {
            name: self.name.clone(),
            control_type: super::OscControlType::Bool,
        }]
    }
}

impl<R: RenderToDmx<bool>> RenderToDmxWithAnimations for Bool<R> {
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        _animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        if self.strobed
            && group_controls.strobe_enabled
            && let Some(state) = group_controls.strobe_shutter()
        {
            self.render.render(&state, dmx_buf);
            return;
        }
        self.render.render(&self.val, dmx_buf);
    }
}

/// Render a bool to fixed values.
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

    #[test]
    fn test_new_off_defaults_false() {
        let ctrl = Bool::new_off("X", ());
        assert!(!ctrl.val());
    }

    #[test]
    fn test_new_on_defaults_true() {
        let ctrl = Bool::new_on("X", ());
        assert!(ctrl.val());
    }

    #[test]
    fn test_control_sets_true() {
        let mut ctrl = Bool::new_off("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/X", OscType::Float(1.0));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert!(ctrl.val());
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        if let OscType::Float(v) = msgs[0].1 {
            assert!((v - 1.0).abs() < 1e-6);
        } else {
            panic!("expected float");
        }
    }

    #[test]
    fn test_control_sets_false() {
        let mut ctrl = Bool::new_on("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/X", OscType::Float(0.0));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert!(!ctrl.val());
    }

    #[test]
    fn test_control_non_matching() {
        let mut ctrl = Bool::new_off("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Y", OscType::Float(1.0));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(!handled);
    }

    #[test]
    fn test_render_bool_to_range() {
        let render = RenderBoolToRange {
            dmx_buf_offset: 0,
            off: 10,
            on: 200,
        };
        let mut buf = [0u8; 1];
        render.render(&true, &mut buf);
        assert_eq!(buf[0], 200);
        render.render(&false, &mut buf);
        assert_eq!(buf[0], 10);
    }
}
