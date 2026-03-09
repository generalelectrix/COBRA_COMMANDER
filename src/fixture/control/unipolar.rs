//! A control for unipolar floats.

use anyhow::Context;
use number::UnipolarFloat;

use crate::{
    channel::KnobIndex,
    osc::{EmitScopedOscMessage, OscControlMessage},
    util::unipolar_to_range,
};

use super::{
    ChannelControl, ChannelKnobHandler, ChannelKnobUnipolar, ChannelLevelHandler,
    ChannelLevelUnipolar, OscControl, RenderToDmx, RenderToDmxWithAnimations,
};

/// A unipolar value, with controls.
#[derive(Debug)]
pub struct Unipolar<R: RenderToDmx<UnipolarFloat>> {
    val: UnipolarFloat,
    name: String,
    render: R,
    strobed: bool,
}

/// A unipolar control that renders into a single DMX channel over a range.
pub type UnipolarChannel = Unipolar<RenderUnipolarToRange>;

impl<R: RenderToDmx<UnipolarFloat>> Unipolar<R> {
    /// Initialize a new control with the provided OSC control name.
    pub fn new<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: UnipolarFloat::ZERO,
            name: name.into(),
            render,
            strobed: false,
        }
    }

    /// Set the initial value of this control to 1.
    pub fn at_full(self) -> Self {
        self.at(UnipolarFloat::ONE)
    }

    /// Set the initial value of this control to the provided value.
    pub fn at(mut self, at: UnipolarFloat) -> Self {
        self.val = at;
        self
    }

    /// Listen to the global strobe clock.
    pub fn strobed(mut self) -> Self {
        self.strobed = true;
        self
    }

    /// Decorate this control with channel level control.
    pub fn with_channel_level(self) -> ChannelLevelUnipolar<Self> {
        ChannelControl::wrap(self, "Level".to_string(), true, ChannelLevelHandler)
    }

    /// Decorate this control with a channel knob of the provided index.
    pub fn with_channel_knob(self, index: KnobIndex) -> ChannelKnobUnipolar<Self> {
        let label = self.name.clone();
        ChannelControl::wrap(self, label, false, ChannelKnobHandler { index })
    }

    /// Get the current value of this control.
    pub fn val(&self) -> UnipolarFloat {
        self.val
    }

    /// Get the current value of this control with animations applied.
    pub fn val_with_anim(&self, animations: impl Iterator<Item = f64>) -> UnipolarFloat {
        let mut val = self.val.val();
        for anim_val in animations {
            // TODO: configurable blend modes
            val += anim_val;
        }
        // TODO: configurable coercing modes
        UnipolarFloat::new(val)
    }
}

impl Unipolar<RenderUnipolarToRange> {
    /// Initialize a unipolar control that renders to a full DMX channel.
    pub fn full_channel<S: Into<String>>(name: S, dmx_buf_offset: usize) -> Self {
        Self::channel(name, dmx_buf_offset, 0, 255)
    }

    /// Initialize a unipolar channel that renders to a partial DMX channel.
    pub fn channel<S: Into<String>>(name: S, dmx_buf_offset: usize, start: u8, end: u8) -> Self {
        Self::new(
            name,
            RenderUnipolarToRange {
                dmx_buf_offset,
                start,
                end,
            },
        )
    }
}

impl<R: RenderToDmx<UnipolarFloat>> OscControl<UnipolarFloat> for Unipolar<R> {
    fn control_direct(
        &mut self,
        val: UnipolarFloat,
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
        self.control_direct(
            msg.get_unipolar().with_context(|| self.name.clone())?,
            emitter,
        )?;
        Ok(true)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&UnipolarFloat),
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
        callback: impl Fn(&UnipolarFloat),
    ) {
        self.emit_state(emitter);
        callback(&self.val);
    }
}

impl<R: RenderToDmx<UnipolarFloat>> super::DescribeOscControls for Unipolar<R> {
    fn describe_controls(&self) -> Vec<super::OscControlDescription> {
        vec![super::OscControlDescription {
            name: self.name.clone(),
            control_type: super::OscControlType::Unipolar,
        }]
    }
}

impl<R: RenderToDmx<UnipolarFloat>> RenderToDmxWithAnimations for Unipolar<R> {
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        if self.strobed
            && group_controls.strobe_enabled
            && let Some(intensity) = group_controls.strobe_intensity()
        {
            self.render.render(&intensity, dmx_buf);
            return;
        }
        self.render.render(&self.val_with_anim(animations), dmx_buf);
    }
}

/// Render a unipolar float to a continuous range.
#[derive(Debug)]
pub struct RenderUnipolarToRange {
    pub dmx_buf_offset: usize,
    pub start: u8,
    pub end: u8,
}

impl RenderToDmx<UnipolarFloat> for RenderUnipolarToRange {
    fn render(&self, val: &UnipolarFloat, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = unipolar_to_range(self.start, self.end, *val);
    }
}

#[cfg(test)]
mod tests {
    use number::UnipolarFloat;
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
    fn test_new_defaults_to_zero() {
        let ctrl = Unipolar::new("X", ());
        assert_eq!(ctrl.val(), UnipolarFloat::ZERO);
    }

    #[test]
    fn test_at_full_sets_to_one() {
        let ctrl = Unipolar::new("X", ()).at_full();
        assert_eq!(ctrl.val(), UnipolarFloat::ONE);
    }

    #[test]
    fn test_at_sets_custom_value() {
        let ctrl = Unipolar::new("X", ()).at(UnipolarFloat::new(0.5));
        assert!((ctrl.val().val() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_control_matching_name() {
        let mut ctrl = Unipolar::new("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/X", OscType::Float(0.75));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert!((ctrl.val().val() - 0.75).abs() < 1e-6);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "X");
    }

    #[test]
    fn test_control_non_matching_name() {
        let mut ctrl = Unipolar::new("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Y", OscType::Float(0.75));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(!handled);
        assert_eq!(ctrl.val(), UnipolarFloat::ZERO);
    }

    #[test]
    fn test_val_with_anim_sums() {
        let ctrl = Unipolar::new("X", ()).at(UnipolarFloat::new(0.3));
        let result = ctrl.val_with_anim([0.2, 0.1].into_iter());
        assert!((result.val() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn test_val_with_anim_clamps() {
        let ctrl = Unipolar::new("X", ()).at(UnipolarFloat::new(0.8));
        let result = ctrl.val_with_anim([0.5].into_iter());
        assert_eq!(result, UnipolarFloat::ONE);
    }

    #[test]
    fn test_render_unipolar_to_range() {
        let render = RenderUnipolarToRange {
            dmx_buf_offset: 0,
            start: 0,
            end: 255,
        };
        let mut buf = [0u8; 1];
        render.render(&UnipolarFloat::new(0.5), &mut buf);
        assert_eq!(buf[0], 127);

        let render = RenderUnipolarToRange {
            dmx_buf_offset: 0,
            start: 100,
            end: 200,
        };
        render.render(&UnipolarFloat::ONE, &mut buf);
        assert_eq!(buf[0], 200);
        render.render(&UnipolarFloat::ZERO, &mut buf);
        assert_eq!(buf[0], 100);
    }
}
