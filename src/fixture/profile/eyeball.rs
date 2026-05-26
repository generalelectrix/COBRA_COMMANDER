//! Spinnaz which themselves are ridin spinnaz - The Eyeball
//!
//! Two-axis-continuous-rotation RGBW LED fat beam
use crate::fixture::{
    color::{Color, Model as ColorRenderModel},
    prelude::*,
};

#[derive(Debug, EmitState, Control, DescribeControls, Update)]
pub struct Eyeball {
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    pan_spin: BipolarSplitChannelMirror,
    #[animate]
    tilt_spin: BipolarSplitChannelMirror,
}

impl PatchFixture for Eyeball {
    const NAME: FixtureType = FixtureType("Eyeball");

    type GroupOptions = super::color::GroupOptions;

    type PatchOptions = ();

    fn new(options: Self::GroupOptions) -> Self {
        Self {
            color: Color::for_subcontrol(None, options.control_color_space),
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(false),
            pan_spin: Bipolar::split_channel("PanSpin", 5, 189, 128, 194, 255, 0)
                .with_mirroring(true),
            tilt_spin: Bipolar::split_channel("TiltSpin", 6, 189, 128, 194, 255, 0)
                .with_mirroring(false),
        }
    }

    fn can_strobe() -> Option<StrobeResponse> {
        Some(StrobeResponse::Short)
    }

    fn new_patch(_: Self::GroupOptions, _: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: 14,
            render_mode: None,
        }
    }
}

impl AnimatedFixture for Eyeball {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.pan.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        );
        dmx_buf[4] = 0; // pan and tilt movement speed, standard (fast)
        self.color.render_for_model(
            ColorRenderModel::Rgbw,
            group_controls,
            &animation_vals.subtarget(),
            dmx_buf,
        );
        dmx_buf[5] = 255; // dimmer always at full, brightness set via color control
        dmx_buf[6] = 0;
        self.pan_spin.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::PanSpin),
            dmx_buf,
        );
        self.tilt_spin.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::TiltSpin),
            dmx_buf,
        );
        self.color.render_for_model(
            ColorRenderModel::Rgb,
            group_controls,
            &animation_vals.subtarget(),
            &mut dmx_buf[7..10],
        );
        dmx_buf[11] = 0; // disable built-in strobe
        dmx_buf[12] = 255; // dimmer at full
        dmx_buf[13] = 0; // disable color-fade macros
    }
}
