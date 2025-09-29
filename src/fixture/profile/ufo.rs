//! Generic Chinese UFO RGBW continuous-rotation moving head
//!
//! For the moment, no knobs for position - we'll rely on pan and tilt sliders,
//! plus the ability to animate them, for now. Might be nice to try an XY pad,
//! but that would require defining a new OSC control type.
use crate::fixture::{
    animation_target::N_ANIM,
    color::{AnimationTarget as ColorAnimationTarget, Color, Model as ColorRenderModel},
    prelude::*,
};

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 16]
pub struct Ufo {
    #[channel_control]
    #[force_osc_control]
    color: Color,
    #[channel_control]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
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
            tilt: Bipolar::coarse_fine("Tily", 2).with_mirroring(true),
        }
    }
}

impl ControllableFixture for Ufo {}

impl AnimatedFixture for Ufo {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
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
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        // Create targeted animation values to pass into the Color control.
        // FIXME: we can surely make this elegant and generalizable.
        let mut color_animation_vals = [(0.0, ColorAnimationTarget::default()); N_ANIM];
        for (i, (val, t)) in animation_vals.iter().copied().enumerate() {
            if let Some(color_target) = t.as_color_target() {
                color_animation_vals[i] = (val, color_target);
            }
        }
        self.color.render_for_model(
            ColorRenderModel::Rgbw,
            group_controls,
            TargetedAnimationValues(&color_animation_vals),
            dmx_buf,
        );
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    strum_macros::EnumString,
    strum_macros::EnumIter,
    strum_macros::Display,
    num_derive::FromPrimitive,
    num_derive::ToPrimitive,
)]
pub enum AnimationTarget {
    #[default]
    Hue,
    Sat,
    Val,
    Rotation,
    Pan,
    Tilt,
}

impl AnimationTarget {
    fn as_color_target(self) -> Option<ColorAnimationTarget> {
        match self {
            Self::Hue => Some(ColorAnimationTarget::Hue),
            Self::Sat => Some(ColorAnimationTarget::Sat),
            Self::Val => Some(ColorAnimationTarget::Val),
            _ => None,
        }
    }
}
