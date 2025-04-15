//! A control for bipolar floats.

use anyhow::Context;
use number::{BipolarFloat, UnipolarFloat};

use crate::{
    channel::KnobIndex,
    osc::{EmitScopedOscMessage, OscControlMessage},
    util::{bipolar_fader_with_detent, unipolar_to_range},
};

use super::{
    Bool, ChannelControl, ChannelKnobBipolar, ChannelKnobHandler, OscControl, RenderToDmx,
    RenderToDmxWithAnimations,
};

/// A bipolar value, with controls.
#[derive(Debug)]
pub struct Bipolar<R: RenderToDmx<BipolarFloat>> {
    val: BipolarFloat,
    // If true, give the input a 5% central null "virtual detent".
    virtual_detent: bool,
    name: String,
    render: R,
}

/// A bipolar control that renders into a single DMX channel over a split range.
pub type BipolarSplitChannel = Bipolar<RenderBipolarToSplitRange>;

/// Bipolar split channel with mirroring.
pub type BipolarSplitChannelMirror = Mirrored<RenderBipolarToSplitRange>;

/// A bipolar control that renders into a single DMX channel over a continuous range.
#[allow(unused)]
pub type BipolarChannel = Bipolar<RenderBipolarToRange>;

/// Bipolar continuous channel with mirroring.
pub type BipolarChannelMirror = Mirrored<RenderBipolarToRange>;

impl<R: RenderToDmx<BipolarFloat>> Bipolar<R> {
    /// Initialize a new control with the provided OSC control name.
    pub fn new<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: BipolarFloat::ZERO,
            virtual_detent: false,
            name: name.into(),
            render,
        }
    }

    /// Use virtual detent with this control.
    pub fn with_detent(mut self) -> Self {
        self.virtual_detent = true;
        self
    }

    /// Decorate this control with automatic mirroring.
    ///
    /// Use the provided bool to determine whether mirroring should be on or off by default.
    pub fn with_mirroring(self, init: bool) -> Mirrored<R> {
        Mirrored::new(self, init)
    }

    /// Decorate this control with a channel knob of the provided index.
    pub fn with_channel_knob(self, index: KnobIndex) -> ChannelKnobBipolar<Self> {
        let label = self.name.clone();
        ChannelControl::wrap(self, label, false, ChannelKnobHandler { index })
    }

    /// Get the current value of this control.
    pub fn val(&self) -> BipolarFloat {
        self.val
    }

    /// Get the current value of this control with animations applied.
    pub fn val_with_anim(&self, animations: impl Iterator<Item = f64>) -> BipolarFloat {
        let mut val = if self.virtual_detent {
            bipolar_fader_with_detent(self.val)
        } else {
            self.val
        }
        .val();
        for anim_val in animations {
            // TODO: configurable blend modes
            val += anim_val;
        }
        BipolarFloat::new(val)
    }
}

impl Bipolar<RenderBipolarToSplitRange> {
    /// Initialize a bipolar control that renders to a split range on a single DMX channel.
    ///
    /// This is the American DJ-style control.
    pub fn split_channel<S: Into<String>>(
        name: S,
        dmx_buf_offset: usize,
        cw_slow: u8,
        cw_fast: u8,
        ccw_slow: u8,
        ccw_fast: u8,
        stop: u8,
    ) -> Self {
        Self::new(
            name,
            RenderBipolarToSplitRange {
                dmx_buf_offset,
                cw_slow,
                cw_fast,
                ccw_slow,
                ccw_fast,
                stop,
            },
        )
    }
}

impl Bipolar<RenderBipolarToRange> {
    /// Initialize a bipolar control that renders to a continuous range on a single DMX channel.
    pub fn channel<S: Into<String>>(name: S, dmx_buf_offset: usize, start: u8, end: u8) -> Self {
        Self::new(
            name,
            RenderBipolarToRange {
                dmx_buf_offset,
                start,
                end,
            },
        )
    }
}

impl<R: RenderToDmx<BipolarFloat>> OscControl<BipolarFloat> for Bipolar<R> {
    fn control_direct(
        &mut self,
        val: BipolarFloat,
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
            msg.get_bipolar().with_context(|| self.name.clone())?,
            emitter,
        )?;
        Ok(true)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&BipolarFloat),
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
        callback: impl Fn(&BipolarFloat),
    ) {
        self.emit_state(emitter);
        callback(&self.val);
    }
}

impl<R: RenderToDmx<BipolarFloat>> RenderToDmxWithAnimations for Bipolar<R> {
    fn render(&self, animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        // TODO: configurable coercing modes
        self.render.render(&self.val_with_anim(animations), dmx_buf);
    }
}

/// Render a bipolar float to a split range.
#[derive(Debug)]
pub struct RenderBipolarToSplitRange {
    pub dmx_buf_offset: usize,
    pub cw_slow: u8,
    pub cw_fast: u8,
    pub ccw_slow: u8,
    pub ccw_fast: u8,
    pub stop: u8,
}

impl RenderToDmx<BipolarFloat> for RenderBipolarToSplitRange {
    fn render(&self, val: &BipolarFloat, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = bipolar_to_split_range(
            *val,
            self.cw_slow,
            self.cw_fast,
            self.ccw_slow,
            self.ccw_fast,
            self.stop,
        );
    }
}

/// Render a bipolar float to a continuous range.
#[derive(Debug)]
pub struct RenderBipolarToRange {
    pub dmx_buf_offset: usize,
    pub start: u8,
    pub end: u8,
}

impl RenderToDmx<BipolarFloat> for RenderBipolarToRange {
    fn render(&self, val: &BipolarFloat, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = bipolar_to_range(self.start, self.end, *val);
    }
}

/// A decorator for bipolar controls that adds configurable mirroring.
///
/// "Mirror" is pre-pended to the inner control name to create the mirror
/// activation toggle.
#[derive(Debug)]
pub struct Mirrored<R: RenderToDmx<BipolarFloat>> {
    control: Bipolar<R>,
    mirror: Bool<()>,
}

impl<R: RenderToDmx<BipolarFloat>> Mirrored<R> {
    /// Decorate a bipolar float control with auto-mirroring.
    ///
    /// If init is true, enable mirroring by default.
    fn new(control: Bipolar<R>, init: bool) -> Self {
        let mirror_name = format!("Mirror{}", control.name);
        Self {
            mirror: if init {
                Bool::new_on(mirror_name, ())
            } else {
                Bool::new_off(mirror_name, ())
            },
            control,
        }
    }

    /// Decorate this control with a channel knob of the provided index.
    pub fn with_channel_knob(self, index: KnobIndex) -> ChannelKnobBipolar<Self> {
        let label = self.control.name.clone();
        ChannelControl::wrap(self, label, false, ChannelKnobHandler { index })
    }
}

impl<R: RenderToDmx<BipolarFloat>> RenderToDmxWithAnimations for Mirrored<R> {
    fn render(&self, animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        // FIXME: should strive to eliminate this code path
        // ignores mirroring when no group controls provided
        self.control.render(animations, dmx_buf);
    }

    fn render_with_group(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        self.control.render.render(
            &self
                .control
                .val_with_anim(animations)
                .invert_if(group_controls.mirror && self.mirror.val()),
            dmx_buf,
        );
    }
}

impl<R: RenderToDmx<BipolarFloat>> OscControl<BipolarFloat> for Mirrored<R> {
    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage) {
        self.control.emit_state(emitter);
        self.mirror.emit_state(emitter);
    }

    fn emit_state_with_callback(
        &self,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&BipolarFloat),
    ) {
        self.control.emit_state_with_callback(emitter, callback);
    }

    fn control_direct(
        &mut self,
        val: BipolarFloat,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        self.control.control_direct(val, emitter)
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.control.control(msg, emitter)? {
            return Ok(true);
        }
        if self.mirror.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&BipolarFloat),
    ) -> anyhow::Result<bool> {
        self.control.control_with_callback(msg, emitter, callback)
    }
}

/// Scale value into the provided integer range.
/// The range is inclusive at both ends.
#[inline(always)]
fn bipolar_to_range(start: u8, end: u8, value: BipolarFloat) -> u8 {
    let uni = UnipolarFloat::new((value.val() + 1.0) / 2.0);
    unipolar_to_range(start, end, uni)
}

/// Scale a bipolar value into an American DJ-style split range.
#[inline(always)]
fn bipolar_to_split_range(
    v: BipolarFloat,
    cw_slow: u8,
    cw_fast: u8,
    ccw_slow: u8,
    ccw_fast: u8,
    stop: u8,
) -> u8 {
    if v == BipolarFloat::ZERO {
        stop
    } else if v.val() > 0.0 {
        unipolar_to_range(cw_slow, cw_fast, v.abs())
    } else {
        unipolar_to_range(ccw_slow, ccw_fast, v.abs())
    }
}
