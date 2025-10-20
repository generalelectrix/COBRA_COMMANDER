//! SGM Colordynamic 575
//! The granddaddy Aquarius.

use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 4]
#[strobe]
pub struct Colordynamic {
    #[channel_control]
    shutter: ChannelLevelBool<BoolChannel>,
    #[channel_control]
    #[animate]
    color_position: ChannelKnobUnipolar<UnipolarChannel>,
    color_rotation_on: Bool<()>,
    #[animate]
    #[channel_control]
    color_rotation_speed: ChannelKnobUnipolar<UnipolarChannel>,
    #[animate]
    #[channel_control]
    fiber_rotation: ChannelKnobBipolar<BipolarSplitChannel>,
}

impl Default for Colordynamic {
    fn default() -> Self {
        Colordynamic {
            shutter: Bool::full_channel("ShutterOpen", 3)
                .strobed()
                .with_channel_level(),
            // shutter: ShutterStrobe::new(
            //     Bool::full_channel("ShutterOpen", 3),
            //     Strobe::channel("Strobe", 3, 16, 239, 255),
            // )
            // .with_channel_level(),
            color_rotation_on: Bool::new_off("ColorRotationOn", ()),
            color_rotation_speed: Unipolar::channel("ColorRotationSpeed", 1, 128, 255)
                .with_channel_knob(1),
            color_position: Unipolar::channel("ColorPosition", 1, 0, 127).with_channel_knob(0),
            fiber_rotation: Bipolar::split_channel("FiberRotation", 2, 113, 0, 142, 255, 128)
                .with_detent()
                .with_channel_knob(2),
        }
    }
}

impl AnimatedFixture for Colordynamic {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        dmx_buf[0] = 0; // FIXME does this do anything?
        if self.color_rotation_on.val() {
            self.color_rotation_speed.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::ColorRotationSpeed),
                dmx_buf,
            );
        } else {
            self.color_position.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::ColorPosition),
                dmx_buf,
            );
        }
        self.fiber_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::FiberRotation),
            dmx_buf,
        );
        self.shutter
            .render(group_controls, std::iter::empty(), dmx_buf);
    }
}
