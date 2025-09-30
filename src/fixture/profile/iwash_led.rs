//! Coemar iWash LED
//!
//! The alien egg sack with the most pastel blue diode of them all. Bleh.
use crate::fixture::{
    animation_target::N_ANIM,
    color::{AnimationTarget as ColorAnimationTarget, Color, Model as ColorRenderModel},
    prelude::*,
};

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 12]
pub struct IWashLed {
    #[channel_control]
    color: Color,
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
}

impl Default for IWashLed {
    fn default() -> Self {
        Self {
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsv),
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(true),
        }
    }
}

impl ControllableFixture for IWashLed {}

impl AnimatedFixture for IWashLed {
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
        dmx_buf[4] = 0; // pan and tilt movement speed, standard (fast)
        dmx_buf[5] = 255; // dimmer always at full, brightness set via color control
        dmx_buf[6] = 0; // TODO: strobe control

        // Create targeted animation values to pass into the Color control.
        // FIXME: we can surely make this elegant and generalizable.
        let mut color_animation_vals = [(0.0, ColorAnimationTarget::default()); N_ANIM];
        for (i, (val, t)) in animation_vals.iter().copied().enumerate() {
            if let Some(color_target) = t.as_color_target() {
                color_animation_vals[i] = (val, color_target);
            }
        }
        self.color.render_for_model(
            ColorRenderModel::Rgb,
            group_controls,
            TargetedAnimationValues(&color_animation_vals),
            &mut dmx_buf[7..10],
        );
        dmx_buf[10] = 0; // useless single white diode "color balance"
        dmx_buf[11] = 0; // fixture reset if set in 101-170
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
