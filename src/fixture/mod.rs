use crate::{
    color::Hsluv,
    master::MasterControls,
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
}

impl<'a> FixtureGroupControls<'a> {
    // TODO: eliminate the need for this method
    pub fn strobe(&self) -> &StrobeState {
        &self.master_controls.strobe_state
    }

    /// Return Some containing a strobe intensity if strobe override is active.
    ///
    /// Return None if we should not be strobing.
    pub fn strobe_intensity(&self, response: StrobeResponse) -> Option<UnipolarFloat> {
        if !self.strobe_enabled {
            return None;
        }
        self.master_controls.strobe_state.intensity(response)
    }

    /// Return Some containing a strobe state if strobe override is active.
    ///
    /// Return None if we should not be strobing.
    pub fn strobe_shutter(&self, response: StrobeResponse) -> Option<bool> {
        self.strobe_intensity(response)
            .map(|i| i > UnipolarFloat::ZERO)
    }
}

pub mod prelude {
    pub use super::fixture::EnumRenderModel;
    pub use super::fixture::{AnimatedFixture, FixtureType, NonAnimatedFixture, Update};
    pub use super::patch::{
        AsPatchOption, CreateAnimatedGroup, CreateNonAnimatedGroup, NoOptions, PatchConfig,
        PatchFixture,
    };
    pub use super::FixtureGroupControls;
    pub use crate::channel::ChannelStateEmitter;
    
    pub use crate::control::EmitControlMessage;
    pub use crate::fixture::animation_target::{Subtarget, TargetedAnimationValues};
    pub use crate::fixture::control::*;
    pub use crate::master::MasterControls;
    pub use crate::osc::prelude::*;
    pub use anyhow::{bail, Result};
    pub use fixture_macros::{
        register_patcher, Control, EmitState, OptionsMenu, PatchFixture, Update,
    };
    pub use number::{BipolarFloat, Phase, UnipolarFloat};
    pub use serde::Deserialize;
}
