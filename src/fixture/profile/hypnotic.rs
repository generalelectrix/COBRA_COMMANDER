//! Intuitive control profile for the American DJ Aquarius 250.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchAnimatedFixture)]
#[channel_count = 2]
pub struct Hypnotic {
    #[channel_control]
    on: ChannelLevelBool<Bool<()>>,
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
            on: Bool::new_off("Shutter", ()).with_channel_level(),
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

impl AnimatedFixture for Hypnotic {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        dmx_buf[0] = if !self.on.control.val() {
            0
        } else {
            match (
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
            }
        };
        self.rotation
            .render_with_group(group_controls, animation_vals.all(), dmx_buf);
    }
}
