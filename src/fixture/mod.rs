use crate::{
    color::Hsluv,
    master::MasterControls,
    preview::FixturePreviewer,
    strobe::{StrobeResponse, StrobeState},
};

pub mod animation_target;
mod control;
#[allow(clippy::module_inception)]
mod fixture;
mod group;
pub mod patch;
mod profile;

pub use fixture::{Control, EmitState, RenderMode};
pub use group::FixtureGroup;
use number::UnipolarFloat;
pub use patch::Patch;
pub use profile::*;

/// Wrap up the master and group-level controls into a single struct to pass
/// into fixtures.
pub struct FixtureGroupControls<'a> {
    /// State of the master controls.
    master_controls: &'a MasterControls,
    /// True if the fixture should render in mirrored mode.
    mirror: bool,
    /// Optional render mode index for fixtures that support more than one.
    render_mode: Option<RenderMode>,
    /// A color value for this fixture to use in rendering.
    color: Option<Hsluv>,
    /// Is master strobing enabled for this group?
    strobe_enabled: bool,
    /// If strobing is enabled, should this fixture be flashing?
    flash_on: bool,
    /// Fixture previewer.
    preview: &'a FixturePreviewer<'a>,
}

impl<'a> FixtureGroupControls<'a> {
    // TODO: eliminate the need for this method
    pub fn strobe(&self) -> &StrobeState {
        &self.master_controls.strobe_state
    }

    /// Return Some containing a strobe intensity if strobe override is active.
    ///
    /// Return None if we should not be strobing.
    pub fn strobe_intensity(&self) -> Option<UnipolarFloat> {
        if !self.strobe_enabled {
            return None;
        }
        Some(if self.flash_on {
            self.master_controls.strobe_state.master_intensity
        } else {
            UnipolarFloat::ZERO
        })
    }

    /// Return Some containing a strobe state if strobe override is active.
    ///
    /// Return None if we should not be strobing.
    pub fn strobe_shutter(&self) -> Option<bool> {
        self.strobe_intensity().map(|i| i > UnipolarFloat::ZERO)
    }
}

pub mod prelude {
    pub use super::fixture::EnumRenderModel;
    pub use super::fixture::{
        AnimatedFixture, FixtureGroupUpdate, FixtureType, NonAnimatedFixture, Update,
    };
    pub use super::patch::{
        AsPatchOption, CreateAnimatedGroup, CreateNonAnimatedGroup, NoOptions, PatchConfig,
        PatchFixture,
    };
    pub use super::FixtureGroupControls;
    pub use crate::channel::ChannelStateEmitter;

    pub use crate::control::EmitControlMessage;
    pub use crate::fixture::animation_target::{Subtarget, TargetedAnimationValues};
    pub use crate::fixture::control::*;
    pub use crate::osc::prelude::*;
    pub use crate::strobe::StrobeResponse;
    pub use anyhow::bail;
    pub use fixture_macros::{
        register_patcher, Control, EmitState, OptionsMenu, PatchFixture, Update,
    };
    pub use number::{BipolarFloat, Phase, UnipolarFloat};
    pub use serde::Deserialize;
}
