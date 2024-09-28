use fixture::FixtureControlMessage;
use std::any::Any;
use std::fmt::Debug;

use self::animation_target::TargetedAnimation;
use crate::animation::ControlMessage as AnimationControlMessage;
use crate::master::{ControlMessage as MasterControlMessage, MasterControls, Strobe};
use crate::osc::{OscClientId, TalkbackMode};
use crate::show::ControlMessage as ShowControlMessage;

pub mod animation_target;
mod fixture;
mod group;
mod patch;
pub mod profile;

pub use fixture::FixtureType;
pub use group::{FixtureGroup, FixtureGroupKey, GroupName};
pub use patch::{Patch, PatchAnimatedFixture, PatchFixture};
pub use profile::*;

#[derive(Debug)]
pub struct ControlMessage {
    pub sender_id: OscClientId,
    pub talkback: TalkbackMode,
    // FIXME: this should be tied to the fixture message payload, not this scope!
    pub key: Option<FixtureGroupKey>,
    pub msg: ControlMessagePayload,
}

#[derive(Debug)]
pub enum ControlMessagePayload {
    Fixture(FixtureControlMessage),
    Master(MasterControlMessage),
    RefreshUI,
    Animation(AnimationControlMessage),
    Show(ShowControlMessage),
}

impl ControlMessagePayload {
    pub fn fixture<T: Any>(msg: T) -> Self {
        Self::Fixture(FixtureControlMessage(Box::new(msg)))
    }
}

pub const N_ANIM: usize = 4;
pub type TargetedAnimations<T> = [TargetedAnimation<T>; N_ANIM];

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
        AnimatedFixture, ControllableFixture, FixtureControlMessage, FixtureType,
        NonAnimatedFixture,
    };
    pub use super::patch::{PatchAnimatedFixture, PatchFixture};
    #[allow(unused)]
    pub use super::FixtureGroupControls;
    pub use crate::fixture::animation_target::TargetedAnimationValues;
    pub use crate::osc::HandleStateChange;
}
