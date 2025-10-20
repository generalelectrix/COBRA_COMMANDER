//! Declarative fixture control models.
//! These types are intended to provide both a data model for fixture state,
//! as well as standardized ways to interact with that state.

use crate::osc::{EmitScopedOscMessage, OscControlMessage};

mod bipolar;
mod bool;
mod channel;
mod indexed_select;
mod labeled_select;
mod phase;
mod strobe;
mod unipolar;

pub use bipolar::*;
pub use bool::*;
pub use channel::*;
pub use indexed_select::*;
pub use labeled_select::*;
pub use phase::*;
pub use strobe::*;
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
