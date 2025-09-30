//! Generic Chinese UFO RGBW continuous-rotation moving head
//!
//! For the moment, no knobs for position - we'll rely on pan and tilt sliders,
//! plus the ability to animate them, for now. Might be nice to try an XY pad,
//! but that would require defining a new OSC control type.
use crate::fixture::{
    color::{Color, Model as ColorRenderModel},
    prelude::*,
};

#[derive(Debug, EmitState, Control, Update, PatchAnimatedFixture)]
#[channel_count = 16]
pub struct Ufo {
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
}

impl Default for Ufo {
    fn default() -> Self {
        Self {
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsv),
            rotation: Bipolar::split_channel("Rotation", 5, 191, 128, 192, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(true),
        }
    }
}

impl AnimatedFixture for Ufo {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.pan.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );
        self.tilt.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        );
        dmx_buf[4] = 255; // pan and tilt movement speed
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        dmx_buf[6] = 255; // dimmer always at full, brightness set via color control
        dmx_buf[7] = 0; // TODO: strobe control

        self.color.render_for_model(
            ColorRenderModel::Rgbw,
            group_controls,
            &animation_vals.subtarget(),
            &mut dmx_buf[8..12],
        );

        // horrible macro channels
        dmx_buf[12] = 0;
        dmx_buf[13] = 0;
        dmx_buf[14] = 0;

        // Remote fixture reset - resets if held at 255 for 5 seconds.
        // TODO: this might be a useful feature to implement if their motion
        // tends to run out of calibration
        dmx_buf[15] = 0;
    }
}
