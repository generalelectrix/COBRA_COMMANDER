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
#[channel_count = 14]
pub struct Ufo {
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    pan: BipolarChannelMirror,
    #[animate]
    tilt: BipolarChannelMirror,
}

impl Default for Ufo {
    fn default() -> Self {
        Self {
<<<<<<< Updated upstream
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsi),
            rotation: Bipolar::split_channel("Rotation", 5, 191, 128, 192, 255, 0)
=======
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsv),
            rotation: Bipolar::split_channel("Rotation", 3, 191, 128, 192, 255, 0)
>>>>>>> Stashed changes
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),
            pan: Bipolar::channel("Pan", 0, 0, 255).with_mirroring(true),
            tilt: Bipolar::channel("Tilt", 1, 0, 255).with_mirroring(true),
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
        dmx_buf[2] = 0; // pan and tilt movement speed
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        dmx_buf[4] = 255; // dimmer always at full, brightness set via color control
        dmx_buf[5] = 0; // TODO: strobe control

        self.color.render_for_model(
            ColorRenderModel::Rgbw,
            group_controls,
            &animation_vals.subtarget(),
            &mut dmx_buf[6..10],
        );

        // horrible macro channels
        dmx_buf[10] = 0;
        dmx_buf[11] = 0;
        dmx_buf[12] = 0;

        // Remote fixture reset - resets if held at 255 for 5 seconds.
        // TODO: this might be a useful feature to implement if their motion
        // tends to run out of calibration
        dmx_buf[13] = 0;
    }
}
