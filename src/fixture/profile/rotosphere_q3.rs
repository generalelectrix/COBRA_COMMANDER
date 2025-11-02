//! Control profle for the Chauvet Rotosphere Q3, aka Son Of Spherion.
use super::color::Model::Rgbw;

use crate::fixture::{color::Color, prelude::*};

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 9]
#[strobe]
pub struct RotosphereQ3 {
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for RotosphereQ3 {
    fn default() -> Self {
        Self {
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsi),
            // strobe: Strobe::channel("Strobe", 4, 1, 250, 0),
            rotation: Bipolar::split_channel("Rotation", 5, 1, 127, 129, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),
        }
    }
}

impl AnimatedFixture for RotosphereQ3 {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.color.render_for_model(
            Rgbw,
            group_controls,
            &animation_vals.subtarget(),
            &mut dmx_buf[0..4],
        );
        dmx_buf[4] = 0; // built-in strobing
        self.rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        dmx_buf[6] = 0;
        dmx_buf[7] = 0;
        dmx_buf[8] = 0;
    }
}
