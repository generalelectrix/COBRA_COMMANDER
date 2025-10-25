//! Control for a generic strobe function.
//!
//! This is intended to model fixtures that provide a specific DMX channel or
//! range for strobe control, and that for one reason or another do not play
//! nicely with the strobe clock feature.

use std::marker::PhantomData;

use anyhow::bail;
use number::UnipolarFloat;

use crate::util::unipolar_to_range;

use super::{Bool, BoolChannel, OscControl, RenderToDmx, RenderToDmxWithAnimations, Unipolar};

/// Generic strobe control, using unipolar rate.
/// Usually also listens to the master strobe control parameter.
#[derive(Debug)]
pub struct Strobe<R: RenderToDmx<Option<UnipolarFloat>>> {
    on: Bool<()>,
    rate: Unipolar<()>,
    render: R,
}

/// A strobe controlling a single basic DMX channel.
pub type StrobeChannel = Strobe<RenderStrobeToRange>;

impl<R: RenderToDmx<Option<UnipolarFloat>>> Strobe<R> {
    pub fn new(name: &str, render: R) -> Self {
        Self {
            on: Bool::new_off(format!("{name}On"), ()),
            rate: Unipolar::new(format!("{name}Rate"), ()),
            render,
        }
    }

    /// Get the current value of this strobe control, if active.
    pub fn val_with_master(&self, master: &crate::strobe::StrobeState) -> Option<UnipolarFloat> {
        let rate = master.rate;

        (self.on.val() && master.strobe_on).then_some(rate)
    }
}

impl StrobeChannel {
    /// Create a strobe that renders to DMX as a single channel, with provided bounds.
    pub fn channel(name: &str, dmx_buf_offset: usize, slow: u8, fast: u8, stop: u8) -> Self {
        Self::new(
            name,
            RenderStrobeToRange {
                dmx_buf_offset,
                slow,
                fast,
                stop,
            },
        )
    }
}

impl<R: RenderToDmx<Option<UnipolarFloat>>> OscControl<()> for Strobe<R> {
    fn control_direct(
        &mut self,
        _val: (),
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        bail!("direct control is not implemented for Strobe controls");
    }

    fn control(
        &mut self,
        msg: &crate::osc::OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.on.control(msg, emitter)? {
            return Ok(true);
        }
        if self.rate.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        self.on.emit_state(emitter);
        self.rate.emit_state(emitter);
    }
}

impl<R: RenderToDmx<Option<UnipolarFloat>>> RenderToDmxWithAnimations for Strobe<R> {
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        _animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        self.render
            .render(&self.val_with_master(group_controls.strobe()), dmx_buf);
    }
}

#[derive(Debug)]
pub struct RenderStrobeToRange {
    dmx_buf_offset: usize,
    slow: u8,
    fast: u8,
    stop: u8,
}

impl RenderToDmx<Option<UnipolarFloat>> for RenderStrobeToRange {
    fn render(&self, val: &Option<UnipolarFloat>, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] = if let Some(rate) = *val {
            unipolar_to_range(self.slow, self.fast, rate)
        } else {
            self.stop
        }
    }
}

/// Combine an arbitrary shutter and strobe control into one logical control.
/// Renders strobe if active, otherwise renders other control.
/// Animations are passed on to the shutter control.
#[derive(Debug)]
pub struct ShutterStrobe<S, R, T>
where
    S: OscControl<T> + RenderToDmxWithAnimations,
    R: RenderToDmx<Option<UnipolarFloat>>,
{
    shutter: S,
    strobe: Strobe<R>,
    phantom: PhantomData<T>,
}

pub type FullShutterStrobe = ShutterStrobe<BoolChannel, RenderStrobeToRange, bool>;

impl<S, R, T> ShutterStrobe<S, R, T>
where
    S: OscControl<T> + RenderToDmxWithAnimations,
    R: RenderToDmx<Option<UnipolarFloat>>,
{
    pub fn new(shutter: S, strobe: Strobe<R>) -> Self {
        Self {
            shutter,
            strobe,
            phantom: PhantomData,
        }
    }
}
impl<S: OscControl<T> + RenderToDmxWithAnimations, R: RenderToDmx<Option<UnipolarFloat>>, T>
    OscControl<T> for ShutterStrobe<S, R, T>
{
    /// Direct control over ShutterStrobe is delegated to the shutter control.
    fn control_direct(
        &mut self,
        val: T,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        self.shutter.control_direct(val, emitter)
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        self.shutter.emit_state(emitter);
        self.strobe.emit_state(emitter);
    }

    /// Callback state emission is delegated to the shutter control.
    fn emit_state_with_callback(
        &self,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
        callback: impl Fn(&T),
    ) {
        self.shutter.emit_state_with_callback(emitter, callback);
        self.strobe.emit_state(emitter);
    }

    fn control(
        &mut self,
        msg: &crate::osc::OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.shutter.control(msg, emitter)? {
            return Ok(true);
        }
        if self.strobe.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }

    /// Control with callback handling is delegated to the shutter control.
    fn control_with_callback(
        &mut self,
        msg: &crate::osc::OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
        callback: impl Fn(&T),
    ) -> anyhow::Result<bool> {
        if self.shutter.control_with_callback(msg, emitter, callback)? {
            return Ok(true);
        }
        if self.strobe.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }
}

impl<S: OscControl<T> + RenderToDmxWithAnimations, R: RenderToDmx<Option<UnipolarFloat>>, T>
    RenderToDmxWithAnimations for ShutterStrobe<S, R, T>
{
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        if let Some(rate) = self.strobe.val_with_master(group_controls.strobe()) {
            self.strobe.render.render(&Some(rate), dmx_buf);
        } else {
            self.shutter.render(group_controls, animations, dmx_buf);
        }
    }
}
