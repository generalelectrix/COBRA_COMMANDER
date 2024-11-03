//! Intuitive control profile for the American DJ Aquarius 250.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control)]
pub struct Hypnotic {
    red_laser_on: Bool<()>,
    green_laser_on: Bool<()>,
    blue_laser_on: Bool<()>,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for Hypnotic {
    fn default() -> Self {
        Self {
            red_laser_on: Bool::new_off("RedLaserOn", ()),
            green_laser_on: Bool::new_off("GreenLaserOn", ()),
            blue_laser_on: Bool::new_off("BlueLaserOn", ()),
            rotation: Bipolar::split_channel("Rotation", 1, 135, 245, 120, 10, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
        }
    }
}

impl PatchAnimatedFixture for Hypnotic {
    const NAME: FixtureType = FixtureType("Hypnotic");
    fn channel_count(&self) -> usize {
        2
    }
}

impl AnimatedFixture for Hypnotic {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        dmx_buf[0] = match (
            self.red_laser_on.val(),
            self.green_laser_on.val(),
            self.blue_laser_on.val(),
        ) {
            (false, false, false) => 0,
            (true, false, false) => 8,
            (false, true, false) => 68,
            (false, false, true) => 128,
            (true, true, false) => 38,
            (true, false, true) => 158,
            (false, true, true) => 98,
            (true, true, true) => 188,
        };
        self.rotation
            .render_with_group(group_controls, animation_vals.all(), dmx_buf);
    }
}

impl ControllableFixture for Hypnotic {}
