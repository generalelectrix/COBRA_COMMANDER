//! Control profile for a uv_led_brick.

use anyhow::Context;
use num_derive::{FromPrimitive, ToPrimitive};
use number::UnipolarFloat;
use strum_macros::{Display as EnumDisplay, EnumIter, EnumString};

use super::prelude::*;
use crate::util::unipolar_to_range;

#[derive(Default, Debug)]
pub struct UvLedBrick(UnipolarFloat);

impl PatchAnimatedFixture for UvLedBrick {
    const NAME: FixtureType = FixtureType("uv_led_brick");
    fn channel_count(&self) -> usize {
        7
    }
}

impl UvLedBrick {
    fn handle_state_change(
        &mut self,
        sc: StateChange,
        emitter: &mut dyn crate::osc::EmitControlMessage,
    ) {
        self.0 = sc;
        Self::emit(sc, emitter);
    }
}

impl AnimatedFixture for UvLedBrick {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
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
    fn emit_state(&self, emitter: &mut dyn crate::osc::EmitControlMessage) {
        Self::emit(self.0, emitter);
    }

    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn crate::osc::EmitControlMessage,
    ) -> anyhow::Result<()> {
        self.handle_state_change(
            *msg.unpack_as::<ControlMessage>().context(Self::NAME)?,
            emitter,
        );
        Ok(())
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
