//! Control profile for a uv_led_brick.

use num_derive::{FromPrimitive, ToPrimitive};
use number::UnipolarFloat;
use strum_macros::{Display as EnumDisplay, EnumIter, EnumString};

use super::{
    AnimatedFixture, ControllableFixture, EmitFixtureStateChange, FixtureControlMessage,
    PatchAnimatedFixture,
};
use crate::{master::MasterControls, util::unipolar_to_range};

#[derive(Default, Debug)]
pub struct UvLedBrick(UnipolarFloat);

impl PatchAnimatedFixture for UvLedBrick {
    const NAME: &'static str = "uv_led_brick";
    fn channel_count(&self) -> usize {
        7
    }
}

impl UvLedBrick {
    fn handle_state_change(&mut self, sc: StateChange, emitter: &mut dyn EmitFixtureStateChange) {
        self.0 = sc;
        emitter.emit_uv_led_brick(sc);
    }
}

impl AnimatedFixture for UvLedBrick {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _master: &MasterControls,
        animation_vals: &super::animation_target::TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let mut level = self.0.val();

        for (val, _target) in animation_vals {
            level += val;
        }
        dmx_buf[0] = unipolar_to_range(0, 255, UnipolarFloat::new(level));
        dmx_buf[4] = 255;
        dmx_buf[5] = 255;
        dmx_buf[6] = 255;
    }
}

impl ControllableFixture for UvLedBrick {
    fn emit_state(&self, emitter: &mut dyn EmitFixtureStateChange) {
        emitter.emit_uv_led_brick(self.0);
    }

    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitFixtureStateChange,
    ) -> Option<FixtureControlMessage> {
        match msg {
            FixtureControlMessage::UvLedBrick(msg) => {
                self.handle_state_change(msg, emitter);
                None
            }
            other => Some(other),
        }
    }
}

pub type StateChange = UnipolarFloat;

// Venus has no controls that are not represented as state changes.
pub type ControlMessage = StateChange;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    EnumString,
    EnumIter,
    EnumDisplay,
    FromPrimitive,
    ToPrimitive,
)]
pub enum AnimationTarget {
    #[default]
    Level,
}

impl AnimationTarget {
    /// Return true if this target is unipolar instead of bipolar.
    #[allow(unused)]
    pub fn is_unipolar(&self) -> bool {
        true
    }
}
