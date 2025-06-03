//! Flexible control profile for a single-color fixture.

use std::collections::HashMap;

use anyhow::{bail, Result};
use log::error;
use strum_macros::VariantArray;

use crate::{
    fixture::{fixture::EnumRenderModel, prelude::*},
    osc::OscControlMessage,
};

#[derive(Debug, Control, EmitState)]
pub struct Color {
    #[channel_control]
    #[animate]
    hue: ChannelKnobPhase<PhaseControl<()>>,
    #[channel_control]
    #[animate]
    sat: ChannelKnobUnipolar<Unipolar<()>>,
    #[channel_control]
    #[animate]
    val: ChannelLevelUnipolar<Unipolar<()>>,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            hue: PhaseControl::new("Hue", ()).with_channel_knob(0),
            sat: Unipolar::new("Sat", ()).at_full().with_channel_knob(1),
            val: Unipolar::new("Val", ()).with_channel_level(),
        }
    }
}

impl PatchAnimatedFixture for Color {
    const NAME: FixtureType = FixtureType("Color");
    fn channel_count(&self, render_mode: Option<crate::fixture::RenderMode>) -> usize {
        Model::model_for_mode(render_mode).unwrap().channel_count()
    }

    fn new(options: &HashMap<String, String>) -> Result<(Self, Option<RenderMode>)> {
        let render_mode = if let Some(kind) = options.get("kind") {
            let model = match kind.as_str() {
                "rgb" => Model::Rgb,
                "DimmerRgb" => Model::DimmerRgb,
                "rgbw" => Model::Rgbw,
                "DimmerRgbw" => Model::DimmerRgbw,
                "hsv" => Model::Hsv,
                "rgbwau" => Model::Rgbwau,
                other => {
                    bail!("unknown color model \"{}\"", other);
                }
            };
            Some(model.render_mode())
        } else {
            None
        };
        Ok((Self::default(), render_mode))
    }
}

crate::register!(Color);

impl Color {
    pub fn render_without_animations(&self, model: Model, dmx_buf: &mut [u8]) {
        model.render(
            dmx_buf,
            self.hue.control.val(),
            self.sat.control.val(),
            self.val.control.val(),
        );
    }
}

impl AnimatedFixture for Color {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let mut hue = self.hue.control.val().val();
        let mut sat = self.sat.control.val().val();
        let mut val = self.val.control.val().val();
        for (anim_val, target) in animation_vals.iter() {
            use AnimationTarget::*;
            match target {
                Hue => hue += anim_val,
                // FIXME: might want to do something nicer for unipolar values
                Sat => sat += anim_val,
                Val => val += anim_val,
            }
        }
        let model = match Model::model_for_mode(group_controls.render_mode) {
            Ok(m) => m,
            Err(err) => {
                error!("failed to render Color: {err}");
                return;
            }
        };
        model.render(
            dmx_buf,
            Phase::new(hue),
            UnipolarFloat::new(sat),
            UnipolarFloat::new(val),
        );
    }
}

impl ControllableFixture for Color {}

impl OscControl<()> for Color {
    fn control_direct(
        &mut self,
        _val: (),
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        bail!("direct control is not implemented for Color controls");
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.hue.control.control(msg, emitter)? {
            return Ok(true);
        }
        if self.sat.control.control(msg, emitter)? {
            return Ok(true);
        }
        if self.val.control.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        self.hue.control.emit_state(emitter);
        self.sat.control.emit_state(emitter);
        self.val.control.emit_state(emitter);
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, VariantArray)]
pub enum Model {
    #[default]
    Rgb,
    DimmerRgb,
    Rgbw,
    DimmerRgbw,
    Hsv,
    Rgbwau,
}

impl EnumRenderModel for Model {}

impl Model {
    fn channel_count(&self) -> usize {
        match self {
            Self::Rgb => 3,
            Self::DimmerRgb => 4,
            Self::Rgbw => 4,
            Self::DimmerRgbw => 5,
            Self::Hsv => 3,
            Self::Rgbwau => 6,
        }
    }

    pub fn render(&self, buf: &mut [u8], hue: Phase, sat: UnipolarFloat, val: UnipolarFloat) {
        match self {
            Self::DimmerRgb => {
                buf[0] = 255;
                let [r, g, b] = hsv_to_rgb(hue, sat, val);
                buf[1] = r;
                buf[2] = g;
                buf[3] = b;
            }
            Self::Rgb => {
                let [r, g, b] = hsv_to_rgb(hue, sat, val);
                buf[0] = r;
                buf[1] = g;
                buf[2] = b;
            }
            Self::Rgbw => {
                let rgb_slice = &mut buf[0..3];
                rgb_slice.copy_from_slice(&hsv_to_rgb(hue, sat, val));
                buf[3] = unit_to_u8((sat.invert() * val).val());
            }
            Self::DimmerRgbw => {
                buf[0] = 255;
                let rgb_slice = &mut buf[1..4];
                rgb_slice.copy_from_slice(&hsv_to_rgb(hue, sat, val));
                buf[4] = unit_to_u8((sat.invert() * val).val());
            }
            Self::Hsv => {
                buf[0] = unit_to_u8(hue.val());
                buf[1] = unit_to_u8(sat.val());
                buf[2] = unit_to_u8(val.val());
            }
            Self::Rgbwau => {
                // TODO: decide what to do with those other diodes...
                let rgb_slice = &mut buf[0..3];
                rgb_slice.copy_from_slice(&hsv_to_rgb(hue, sat, val));
            }
        }
    }
}

type ColorRgb = [u8; 3];

fn hsv_to_rgb(hue: Phase, sat: UnipolarFloat, val: UnipolarFloat) -> ColorRgb {
    if sat == 0.0 {
        let v = unit_to_u8(val.val());
        return [v, v, v];
    }
    let var_h = if hue == 1.0 { 0.0 } else { hue.val() * 6.0 };

    let var_i = var_h.floor();
    let var_1 = val.val() * (1.0 - sat.val());
    let var_2 = val.val() * (1.0 - sat.val() * (var_h - var_i));
    let var_3 = val.val() * (1.0 - sat.val() * (1.0 - (var_h - var_i)));

    let (rv, gv, bv) = match var_i as i64 {
        0 => (val.val(), var_3, var_1),
        1 => (var_2, val.val(), var_1),
        2 => (var_1, val.val(), var_3),
        3 => (var_1, var_2, val.val()),
        4 => (var_3, var_1, val.val()),
        _ => (val.val(), var_1, var_2),
    };
    [unit_to_u8(rv), unit_to_u8(gv), unit_to_u8(bv)]
}

fn unit_to_u8(v: f64) -> u8 {
    (255. * v).round() as u8
}
