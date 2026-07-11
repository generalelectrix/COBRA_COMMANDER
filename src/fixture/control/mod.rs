//! Declarative fixture control models.
//! These types are intended to provide both a data model for fixture state,
//! as well as standardized ways to interact with that state.

use number::BipolarFloat;

use crate::osc::{EmitScopedOscMessage, OscControlMessage};

mod bipolar;
mod bool;
mod channel;
mod indexed_select;
mod labeled_select;
mod phase;
pub mod strobe_array;
mod strobe_follower;
mod unipolar;

pub use bipolar::*;
pub use bool::*;
pub use channel::*;
pub use indexed_select::*;
pub use labeled_select::*;
pub use phase::*;
pub use strobe_follower::*;
pub use unipolar::*;

use super::FixtureGroupControls;

pub trait OscControl<T> {
    /// Set this control directly with the provided value.
    fn control_direct(&mut self, val: T, emitter: &dyn EmitScopedOscMessage) -> anyhow::Result<()>;

    /// Potentially handle an OSC control message.
    /// If we handle the message, return true.
    /// If we don't handle the message, return false.
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<bool>;

    /// Potentially handle an OSC control message.
    /// If we handle the message, return true.
    /// If we don't handle the message, return false.
    ///
    /// Call the provided callback to emit a new version of the current value
    /// if a control message was handled.
    #[allow(unused)]
    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&T),
    ) -> anyhow::Result<bool> {
        // Default implementation ignores a provided callback.
        self.control(msg, emitter)
    }

    /// Emit the current state of this control.
    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage);

    /// Emit the current state of this control.
    ///
    /// Call the provided callback with the current state.
    #[allow(unused)]
    fn emit_state_with_callback(&self, emitter: &dyn EmitScopedOscMessage, callback: impl Fn(&T)) {
        // Default implementation ignores a provided callback.
        self.emit_state(emitter);
    }
}

pub trait RenderToDmxWithAnimations {
    /// Render a control into a DMX buffer.
    ///
    /// Handle animation values if any are provided. Also potentially make use
    /// of the group controls.
    fn render(
        &self,
        group_controls: &FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    );
}

pub trait RenderToDmx<T> {
    /// Render a value into a DMX buffer using some strategy.
    fn render(&self, val: &T, dmx_buf: &mut [u8]);
}

/// A render strategy that does nothing.
/// Used for controls which themselves are not rendered directly to DMX.
impl<T> RenderToDmx<T> for () {
    fn render(&self, _val: &T, _dmx_buf: &mut [u8]) {}
}

/// Decorates a bipolar render strategy with a calibration offset.
///
/// Recenters the control on `offset` — input `0` renders as `offset` — and
/// rescales the bipolar range symmetrically about it, filling to the nearer
/// rail (the half-span becomes `1 - |offset|`). This calibrates out a
/// fixed physical offset while still using the full fader throw: both ends pan
/// as far as possible in each direction, limited by the now-smaller side.
///
/// Placement matters. This must wrap the base render strategy — the innermost,
/// terminal render step — so the remap is the last transformation applied, after
/// any earlier value processing. That makes it a fixed calibration of the
/// rendered output. Applied to an outer value transform it would instead remap
/// the control's input, which a later transform could then alter — not a stable
/// calibration. Apply it first, before any other decorator.
#[derive(Debug)]
pub struct OffsetRender<R> {
    offset: BipolarFloat,
    inner: R,
}

impl<R> OffsetRender<R> {
    pub fn new(offset: BipolarFloat, inner: R) -> Self {
        Self { offset, inner }
    }
}

impl<R: RenderToDmx<BipolarFloat>> RenderToDmx<BipolarFloat> for OffsetRender<R> {
    fn render(&self, val: &BipolarFloat, dmx_buf: &mut [u8]) {
        // Recenter on `offset` and rescale the range symmetrically about it,
        // filling to the nearer rail (half-span 1 - |offset|). Stays in range by
        // construction; clamp guards against float error.
        let offset = self.offset.val();
        let scaled = offset + val.val() * (1.0 - offset.abs());
        self.inner.render(&BipolarFloat::new(scaled), dmx_buf);
    }
}

/// Reverses a bipolar control's handedness by negating the value before
/// delegating to the wrapped render strategy, so the same input drives the
/// opposite direction.
///
/// Like [`OffsetRender`], this wraps the base render strategy — the innermost,
/// terminal render step — so the negation is a fixed property of the rendered
/// output rather than an input transform a later decorator could alter.
#[derive(Debug)]
pub struct InvertRender<R> {
    inner: R,
}

impl<R> InvertRender<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: RenderToDmx<BipolarFloat>> RenderToDmx<BipolarFloat> for InvertRender<R> {
    fn render(&self, val: &BipolarFloat, dmx_buf: &mut [u8]) {
        self.inner.render(&val.invert(), dmx_buf);
    }
}

/// The kinds of OSC controls that fixtures can expose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscControlType {
    /// Float 0.0 to 1.0.
    Unipolar,
    /// Float -1.0 to 1.0.
    Bipolar,
    /// On/off toggle.
    Bool,
    /// Select from a menu of labeled choices.
    LabeledSelect { labels: Vec<&'static str> },
    /// Select from a numeric index range (radio button grid).
    IndexedSelect {
        n: usize,
        /// If true, the x coordinate is the primary (index) coordinate.
        x_primary_coordinate: bool,
    },
    /// Phase 0.0 to 1.0.
    Phase,
}

impl std::fmt::Display for OscControlType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unipolar => f.write_str("unipolar (0.0-1.0)"),
            Self::Bipolar => f.write_str("bipolar (-1.0-1.0)"),
            Self::Bool => f.write_str("bool"),
            Self::LabeledSelect { labels } => write!(f, "select [{}]", labels.join(", ")),
            Self::IndexedSelect { n, .. } => write!(f, "select (1-{n})"),
            Self::Phase => f.write_str("phase (0.0-1.0)"),
        }
    }
}

/// A single OSC control: its name and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscControlDescription {
    pub name: String,
    pub control_type: OscControlType,
}

/// Describe the OSC controls exposed by this type.
///
/// This is an instance method because control names are set at runtime
/// (in Default::default impls), not at the type level.
pub trait DescribeOscControls {
    /// Return descriptions of all OSC controls this value exposes.
    fn describe_controls(&self) -> Vec<OscControlDescription>;
}
