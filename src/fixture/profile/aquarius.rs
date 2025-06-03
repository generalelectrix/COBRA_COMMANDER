//! Intuitive control profile for the American DJ Aquarius 250.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 2]
pub struct Aquarius {
    #[channel_control]
    lamp_on: ChannelLevelBool<BoolChannel>,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for Aquarius {
    fn default() -> Self {
        Self {
            lamp_on: Bool::full_channel("LampOn", 1).with_channel_level(),
            rotation: Bipolar::split_channel("Rotation", 0, 130, 8, 132, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
        }
    }
}

impl AnimatedFixture for Aquarius {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.rotation
            .render_with_group(group_controls, animation_vals.all(), dmx_buf);
        self.lamp_on
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
    }
}

impl ControllableFixture for Aquarius {}
