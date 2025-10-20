//! A control for unipolar floats.

use anyhow::Context;
use number::UnipolarFloat;

use crate::{
    channel::KnobIndex,
    osc::{EmitScopedOscMessage, OscControlMessage},
    strobe::StrobeResponse,
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
    strobed: Option<StrobeResponse>,
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
            strobed: None,
        }
    }

    /// Set the initial value of this control to 1.
    pub fn at_full(mut self) -> Self {
        self.val = UnipolarFloat::ONE;
        self
    }

    /// Listen to the global strobe clock, short pulse width.
    pub fn strobed_short(mut self) -> Self {
        self.strobed = Some(StrobeResponse::Short);
        self
    }

    /// Listen to the global strobe clock, long pulse width.
    pub fn strobed_long(mut self) -> Self {
        self.strobed = Some(StrobeResponse::Long);
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

impl<R: RenderToDmx<UnipolarFloat>> RenderToDmxWithAnimations for Unipolar<R> {
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        if group_controls.strobe_enabled {
            if let Some(response) = self.strobed {
                if let Some(intensity) = group_controls.strobe_intensity(response) {
                    self.render.render(&intensity, dmx_buf);
                    return;
                }
            }
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
