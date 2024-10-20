use crate::master::{MasterControls, Strobe};

pub mod animation_target;
mod control;
#[allow(clippy::module_inception)]
mod fixture;
mod group;
mod patch;
mod profile;

pub use group::{FixtureGroup, FixtureGroupKey, GroupName};
pub use patch::Patch;
pub use profile::*;

/// Wrap up the master and group-level controls into a single struct to pass
/// into fixtures.
pub struct FixtureGroupControls<'a> {
    /// Master controls.
    master_controls: &'a MasterControls,
    /// True if the fixture should render in mirrored mode.
    mirror: bool,
}

impl<'a> FixtureGroupControls<'a> {
    pub fn strobe(&self) -> &Strobe {
        self.master_controls.strobe()
    }
}

pub mod prelude {
    pub use super::fixture::{
        AnimatedFixture, ControllableFixture, FixtureType, NonAnimatedFixture,
    };
    pub use super::patch::{PatchAnimatedFixture, PatchFixture};
    pub use super::FixtureGroupControls;
    pub use crate::channel::{ChannelControlMessage, ChannelStateChange, ChannelStateEmitter};
    pub use crate::fixture::animation_target::TargetedAnimationValues;
    pub use crate::fixture::control::*;
    pub use crate::fixture::generic::*;
    pub use anyhow::{bail, Result};
    pub use number::{BipolarFloat, Phase, UnipolarFloat};
}
