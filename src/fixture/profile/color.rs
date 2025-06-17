//! Flexible control profile for a single-color fixture.
//! Supports several color space options:

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use colored::Colorize;
use hsluv::hsluv_to_rgb;
use log::{error, warn};
use strum_macros::{EnumString, VariantArray};

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
    /// Extra third knob for controlling HSLuv; set to zero, this sets the
    /// overall lightness to the value that includes all primary colors in the
    /// output gamut.
    /// Larger values span the rest of the lightness range.
    #[channel_control]
    #[optional]
    lightness_boost: Option<ChannelKnobUnipolar<Unipolar<()>>>,

    #[skip_control]
    #[skip_emit]
    space: ColorSpace,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            hue: PhaseControl::new("Hue", ()).at_half().with_channel_knob(0),
            sat: Unipolar::new("Sat", ()).at_full().with_channel_knob(1),
            val: Unipolar::new("Val", ()).with_channel_level(),
            lightness_boost: None,
            space: ColorSpace::Hsv,
        }
    }
}

impl PatchAnimatedFixture for Color {
    const NAME: FixtureType = FixtureType("Color");
    fn channel_count(&self, render_mode: Option<crate::fixture::RenderMode>) -> usize {
        Model::model_for_mode(render_mode).unwrap().channel_count()
    }

    fn new(options: &HashMap<String, String>) -> Result<(Self, Option<RenderMode>)> {
        let render_mode = options
            .get("kind")
            .map(|kind| {
                kind.parse::<Model>()
                    .with_context(|| format!("unknown color output model \"{kind}\""))
            })
            .transpose()?
            .unwrap_or_default()
            .render_mode();
        let space = options
            .get("control_color_space")
            .map(|space| {
                space
                    .parse::<ColorSpace>()
                    .with_context(|| format!("unknown color control space \"{space}\""))
            })
            .transpose()?
            .unwrap_or_default();
        let mut c = Self {
            space,
            ..Default::default()
        };
        if space == ColorSpace::Hsluv {
            c.lightness_boost = Some(Unipolar::new("LightnessBoost", ()).with_channel_knob(2));
        }
        Ok((c, Some(render_mode)))
    }
}

crate::register!(Color);

const HSLUV_LIGHTNESS_OFFSET: UnipolarFloat = UnipolarFloat::new(0.3225);
const HSLUV_LIGHTNESS_BOOST_SCALE: UnipolarFloat = UnipolarFloat::new(1.0 - 0.3225);

impl Color {
    /// Return a lightness value for HSLuv.
    /// Return 0 if we unexpectedly don't have a lightness boost control configured.
    /// This does NOT include the rescaling from the overall level fader.
    fn hsluv_lightness(&self) -> UnipolarFloat {
        let Some(lightness_boost) = &self.lightness_boost else {
            error!("No lightness boost control configured for HSLuv color.");
            return UnipolarFloat::ZERO;
        };

        HSLUV_LIGHTNESS_OFFSET + HSLUV_LIGHTNESS_BOOST_SCALE * lightness_boost.control.val()
    }

    pub fn render_without_animations(&self, model: Model, dmx_buf: &mut [u8]) {
        match self.space {
            ColorSpace::Hsv => model.render(
                dmx_buf,
                HsvRenderer {
                    hue: self.hue.control.val(),
                    sat: self.sat.control.val(),
                    val: self.val.control.val(),
                },
            ),
            ColorSpace::Hsluv => model.render(
                dmx_buf,
                HsluvRenderer {
                    hue: self.hue.control.val(),
                    sat: self.sat.control.val(),
                    lightness: self.hsluv_lightness() * self.val.control.val(),
                },
            ),
        }
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
        match self.space {
            ColorSpace::Hsv => model.render(
                dmx_buf,
                HsvRenderer {
                    hue: Phase::new(hue),
                    sat: UnipolarFloat::new(sat),
                    val: UnipolarFloat::new(val),
                },
            ),
            ColorSpace::Hsluv => model.render(
                dmx_buf,
                HsluvRenderer {
                    hue: Phase::new(hue),
                    sat: UnipolarFloat::new(sat),
                    lightness: self.hsluv_lightness() * UnipolarFloat::new(val),
                },
            ),
        }
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

/// Control and color models for different color spaces.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, EnumString)]
enum ColorSpace {
    /// Standard HSV color space.
    #[default]
    Hsv,
    /// HSLuv perceptually uniform color space.
    /// www.hsluv.org
    Hsluv,
}

/// An entity that can render an abstract color into various output spaces.
pub trait RenderColor {
    fn rgb(&self) -> ColorRgb;
    fn rgbw(&self) -> ColorRgbw;
    fn hsv(&self) -> ColorHsv;
}

/// Render an HSV color into output spaces.
pub struct HsvRenderer {
    pub hue: Phase,
    pub sat: UnipolarFloat,
    pub val: UnipolarFloat,
}

impl RenderColor for HsvRenderer {
    fn rgb(&self) -> ColorRgb {
        hsv_to_rgb(self.hue, self.sat, self.val)
    }
    fn rgbw(&self) -> ColorRgbw {
        let [r, g, b] = self.rgb();
        // FIXME: this is a shitty way to use the white diode.
        // We should rescale the other values to maintain total brightness while
        // bringing in white for pastels. This will take some thinking, and won't
        // work for all colors.
        let w = unit_to_u8((self.sat.invert() * self.val).val());
        [r, g, b, w]
    }
    fn hsv(&self) -> ColorHsv {
        // We set green as hue = 0; everything else has red = 0.  Shift.
        let shifted_hue = self.hue + 1. / 3.;
        [
            unit_to_u8(shifted_hue.val()),
            unit_to_u8(self.sat.val()),
            unit_to_u8(self.val.val()),
        ]
    }
}

/// Render an HSLuv color into output spaces.
pub struct HsluvRenderer {
    pub hue: Phase,
    pub sat: UnipolarFloat,
    pub lightness: UnipolarFloat,
}

impl RenderColor for HsluvRenderer {
    fn rgb(&self) -> ColorRgb {
        // HSLuv library uses degrees for phase and percent for other two components.
        // Primary and secondary hues at base lightness (renders primary blue) are:
        // (r, y, g, c, b, m): (12.2, 85.8, 127.7, 191.6, 265.87, 307.7)
        // Subtracting 127.7 to shift primary green to 0 gives:
        // (r, y, g, c, b, m): (244.5, 318.1, 0.0, 63.9, 138.17, 180.0)

        // Observations:
        // - each primary and opposing secondary are actually 180 degrees apart
        // - the primary to primary distance and primary to secondary distance
        //   varies quite a bit, presumably due to our ability to resolve subtle
        //   hue variations in each range differently. This isn't great for art,
        //   though, since too much of our hue range is dedicated to shades of
        //   green. Rescale the ranges to give each subset an equal share of
        //   the hue range.

        // Perform operations in the unit range.
        const RED: f64 = 244.5 / 360.;
        const GREEN: f64 = 0.;
        const BLUE: f64 = 138.17 / 360.;
        const ONE_THIRD: f64 = 1. / 3.;
        const TWO_THIRD: f64 = 2. / 3.;
        const CYAN_SHIFT: f64 = (BLUE - GREEN) / ONE_THIRD;
        const MAGENTA_SHIFT: f64 = (RED - BLUE) / ONE_THIRD;
        const YELLOW_SHIFT: f64 = (1. - RED) / ONE_THIRD;

        let hue = self.hue.val();

        // Shift hue ranges.
        let rescaled_hue = if hue < ONE_THIRD {
            hue * CYAN_SHIFT
        } else if hue < TWO_THIRD {
            ((hue - ONE_THIRD) * MAGENTA_SHIFT) + BLUE
        } else {
            ((hue - TWO_THIRD) * YELLOW_SHIFT) + RED
        };
        // Convert to degrees and shift up by 127.7 to place green at 0.
        let hue_degrees = rescaled_hue * 360. + 127.7;
        let (r, g, b) = hsluv_to_rgb(
            hue_degrees,
            self.sat.val() * 100.,
            self.lightness.val() * 100.,
        );
        [unit_to_u8(r), unit_to_u8(g), unit_to_u8(b)]
    }

    fn rgbw(&self) -> ColorRgbw {
        // TODO: we have lots of rich info about our input color, we should be
        // able to make good use of the white diode.
        // Inspiration: https://blog.saikoled.com/post/44677718712/how-to-convert-from-hsi-to-rgb-white
        let [r, g, b] = self.rgb();
        [r, g, b, 0]
    }

    fn hsv(&self) -> ColorHsv {
        warn!("HSV output rendering is not implemented for HSLuv");
        [0, 0, 0]
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, EnumString, VariantArray)]
pub enum Model {
    #[default]
    /// RGB in 3 DMX channels.
    Rgb,
    /// Dimmer in first channel + RGB.
    DimmerRgb,
    /// RGBW in 4 DMX channels.
    Rgbw,
    /// Dimmer in first channel + RGBW.
    DimmerRgbw,
    /// HSV in 3 DMX channels.
    Hsv,
    /// RGBWAU in 6 DMX channels.
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

    pub fn render(&self, buf: &mut [u8], renderer: impl RenderColor) {
        match self {
            Self::Rgb => {
                let [r, g, b] = renderer.rgb();
                buf[0] = r;
                buf[1] = g;
                buf[2] = b;
            }
            Self::DimmerRgb => {
                buf[0] = 255;
                Self::Rgb.render(&mut buf[1..], renderer);
            }
            Self::Rgbw => {
                let [r, g, b, w] = renderer.rgbw();
                buf[0] = r;
                buf[1] = g;
                buf[2] = b;
                buf[3] = w;
            }
            Self::DimmerRgbw => {
                buf[0] = 255;
                Self::Rgbw.render(&mut buf[1..], renderer);
            }
            Self::Hsv => {
                let [h, s, v] = renderer.hsv();
                buf[0] = h;
                buf[1] = s;
                buf[2] = v;
            }
            Self::Rgbwau => {
                Self::Rgb.render(&mut buf[0..3], renderer);
                // TODO: decide what to do with those other diodes...
                // Amber probably isn't well standardized, even worse than white.
            }
        }
    }
}

/// Print a brick of color to stdout.
///
/// This can be used for debugging color output.
#[allow(unused)]
fn print_color([r, g, b]: ColorRgb) {
    print!("{}", "▮".truecolor(r, g, b).on_truecolor(r, g, b));
}

/// An HSV color in an output 24-bit space.
/// This is an uncommon output model, but a few models of DMX fixture do use it.
type ColorHsv = [u8; 3];

/// 24-bit RGB color.
/// Most common output color space.
type ColorRgb = [u8; 3];

/// 32-bit RGBW color.
/// Used by LED fixtures with a white diode in addition to RGB.
type ColorRgbw = [u8; 4];

/// Convert unit-scaled HSV into a 24-bit RGB color.
///
/// NOTE: we shift the hue coordinate by 1/3, to put green at zero instead of red.
/// This makes it easy to turn a knob between yellow, red, and magenta without
/// passing through green.
pub fn hsv_to_rgb(hue: Phase, sat: UnipolarFloat, val: UnipolarFloat) -> ColorRgb {
    let hue = hue + 1. / 3.;
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
