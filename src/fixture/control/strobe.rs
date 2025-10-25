//! Control for a generic strobe function.
//!
//! This is intended to model fixtures that provide a specific DMX channel or
//! range for strobe control and that for one reason or another do not play
//! nicely with the strobe clock feature (such as being wireless-only).
//!
//! TODO: we should provide a rescaling feature to attempt to match global rate
//! with a fixture's rate as well as we can. This will take some work...

use number::UnipolarFloat;

use crate::util::unipolar_to_range;

use super::{OscControl, RenderToDmx, RenderToDmxWithAnimations};

/// Generic strobe control, responding to the global strobe clock.
#[derive(Debug)]
pub struct StrobeFollower<R: RenderToDmx<Option<UnipolarFloat>>> {
    render: R,
}

/// A strobe controlling a single basic DMX channel.
pub type StrobeChannel = StrobeFollower<RenderStrobeToRange>;

impl<R: RenderToDmx<Option<UnipolarFloat>>> StrobeFollower<R> {
    pub fn new(render: R) -> Self {
        Self { render }
    }
}

impl StrobeChannel {
    /// Create a strobe that renders to DMX as a single channel, with provided bounds.
    pub fn channel(dmx_buf_offset: usize, slow: u8, fast: u8, stop: u8) -> Self {
        Self::new(RenderStrobeToRange {
            dmx_buf_offset,
            slow,
            fast,
            stop,
        })
    }
}

// Provide a no-op impl of OscControl so we don't need to opt these things
// out of the derive trait.
impl<R: RenderToDmx<Option<UnipolarFloat>>> OscControl<()> for StrobeFollower<R> {
    fn control_direct(
        &mut self,
        _val: (),
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn control(
        &mut self,
        _msg: &crate::osc::OscControlMessage,
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn emit_state(&self, _emitter: &dyn crate::osc::EmitScopedOscMessage) {}
}

impl<R: RenderToDmx<Option<UnipolarFloat>>> RenderToDmxWithAnimations for StrobeFollower<R> {
    fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        _animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        let rate = (group_controls.strobe_enabled && group_controls.strobe().strobe_on)
            .then(|| group_controls.strobe().rate);
        self.render.render(&rate, dmx_buf);
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
