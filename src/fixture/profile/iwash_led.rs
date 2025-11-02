//! Coemar iWash LED
//!
//! The alien egg sack with the most pastel blue diode of them all. Bleh.
use crate::fixture::{
    color::{Color, Model as ColorRenderModel},
    prelude::*,
};

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 12]
#[strobe_external]
pub struct IWashLed {
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
}

impl Default for IWashLed {
    fn default() -> Self {
        Self {
            color: Color::for_subcontrol(None, crate::color::ColorSpace::Hsluv),
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(true),
        }
    }
}

impl AnimatedFixture for IWashLed {
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
        dmx_buf[5] = 255; // dimmer always at full, brightness set via color control
        dmx_buf[6] = 0; // TODO: strobe control

        self.color.render_for_model(
            ColorRenderModel::Rgb,
            group_controls,
            &animation_vals.subtarget(),
            &mut dmx_buf[7..10],
        );
        dmx_buf[10] = 0; // useless single white diode "color balance"
        dmx_buf[11] = 0; // fixture reset if set in 101-170
    }
}
