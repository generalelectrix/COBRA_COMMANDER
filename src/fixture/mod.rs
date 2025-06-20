use crate::{
    color::Hsluv,
    master::{MasterControls, Strobe},
};

pub mod animation_target;
mod control;
#[allow(clippy::module_inception)]
mod fixture;
mod group;
mod patch;
mod profile;

pub use fixture::{Control, EmitState, RenderMode};
pub use group::{FixtureGroup, FixtureGroupKey};
pub use patch::Patch;
pub use profile::*;

/// Wrap up the master and group-level controls into a single struct to pass
/// into fixtures.
pub struct FixtureGroupControls<'a> {
    /// Master controls.
    master_controls: &'a MasterControls,
    /// True if the fixture should render in mirrored mode.
    mirror: bool,
    /// Optional render mode index for fixtures that support more than one.
    render_mode: Option<RenderMode>,
    /// A color value for this fixture to use in rendering.
    color: Option<Hsluv>,
}

impl<'a> FixtureGroupControls<'a> {
    pub fn strobe(&self) -> Strobe {
        self.master_controls.strobe()
    }
}

pub mod prelude {
    pub use super::fixture::{
        AnimatedFixture, ControllableFixture, FixtureType, NonAnimatedFixture, RenderMode,
    };
    pub use super::patch::{PatchAnimatedFixture, PatchFixture};
    pub use super::FixtureGroupControls;
    pub use crate::channel::ChannelStateEmitter;
    pub use crate::control::EmitControlMessage;
    pub use crate::fixture::animation_target::TargetedAnimationValues;
    pub use crate::fixture::control::*;
    pub use crate::fixture::generic::*;
    pub use crate::master::MasterControls;
    pub use crate::osc::prelude::*;
    pub use anyhow::bail;
    pub use fixture_macros::{
        register_patcher, Control, EmitState, PatchAnimatedFixture, PatchFixture,
    };
    pub use number::{BipolarFloat, Phase, UnipolarFloat};
}
