//! Things related to color spaces, color models, rendering colors, etc.

use std::f64::consts::PI;

use colored::Colorize;
use fixture_macros::AsPatchOption;
use hsluv::hsluv_to_rgb;
use log::warn;
use number::{Phase, UnipolarFloat};
use serde::Deserialize;
use strum_macros::{Display, EnumIter};

/// Supported control color spaces.
#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, Display, EnumIter, AsPatchOption,
)]
pub enum ColorSpace {
    /// HSV color space with green shifted to hue = 0.
    #[default]
    Hsv,
    /// HSI color space with green shifted to hue = 0.
    Hsi,
    /// HSLuv perceptually uniform color space, green shifted to hue = 0.
    /// www.hsluv.org
    ///
    /// Hue coordinates are slightly re-scaled to put primaries exactly 120
    /// degrees apart.
    Hsluv,
}

/// Render a specific color out into various integer-based output spaces.
pub trait RenderColor {
    fn rgb(&self) -> ColorRgb;
    fn rgbw(&self) -> ColorRgbw;
    fn hsv(&self) -> ColorHsv;
}

/// A color in the HSV color space.
///
/// The hue coordinate is adjusted to put green at 0.
#[derive(Clone)]
pub struct Hsv {
    pub hue: Phase,
    pub sat: UnipolarFloat,
    pub val: UnipolarFloat,
}

impl RenderColor for Hsv {
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
        // This controller defines green as hue = 0.
        // Shift hue back into the standard HSV expectation where the hue for
        // red = 0.
        let shifted_hue = self.hue + 1. / 3.;
        [
            unit_to_u8(shifted_hue.val()),
            unit_to_u8(self.sat.val()),
            unit_to_u8(self.val.val()),
        ]
    }
}

/// A color in the HSI color space.
///
/// The hue coordinate is adjusted to put green at 0.
/// HSI is a compromise between HSV and HSLuv - it is not perceptually uniform,
/// but maintains "constant power" across all emitters for a given I.
///
/// The RGBW render implementation also makes effective use of the W diode
/// under the assumption that its emission power is comparable to the other
/// diodes, ensuring that any given pastel color uses no more than two of the
/// three RGB emitters, and uses only the W diode when fully desaturated. This
/// is a particularly useful color space for RGB or RGBW beam fixtures that do
/// not actually color mix the diodes.
#[derive(Clone)]
pub struct Hsi {
    pub hue: Phase,
    pub sat: UnipolarFloat,
    pub intensity: UnipolarFloat,
}

impl RenderColor for Hsi {
    fn rgb(&self) -> ColorRgb {
        hsi_to_rgb(self.hue, self.sat, self.intensity)
    }
    fn rgbw(&self) -> ColorRgbw {
        hsi_to_rgbw(self.hue, self.sat, self.intensity)
    }
    fn hsv(&self) -> ColorHsv {
        unimplemented!("conversion from HSI to HSV is not implemented");
    }
}

/// A color in the HSLuv color space.
///
/// The behavior of hue is tweaked compared to the reference implementation.
/// Green is at hue = 0.
/// The primaries are adjusted to be exactly 120 degrees apart.
#[derive(Clone, Debug)]
pub struct Hsluv {
    pub hue: Phase,
    pub sat: UnipolarFloat,
    pub lightness: UnipolarFloat,
}

impl RenderColor for Hsluv {
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

/// This is the lightness value where the gamut contains all three primary colors
/// at the brightness equivalent to blue at maximum output.
pub const HSLUV_LIGHTNESS_OFFSET: UnipolarFloat = UnipolarFloat::new(0.3225);

/// An HSV color in an output 24-bit space.
/// This is an uncommon output model, but a few models of DMX fixture do use it.
pub type ColorHsv = [u8; 3];

/// 24-bit RGB color.
/// Most common output color space.
pub type ColorRgb = [u8; 3];

/// 32-bit RGBW color.
/// Used by LED fixtures with a white diode in addition to RGB.
pub type ColorRgbw = [u8; 4];

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

/// Convert unit-scaled HSI into a 24-bit RGB color.
///
/// NOTE: we shift the hue coordinate by 1/3, to put green at zero instead of red.
/// This makes it easy to turn a knob between yellow, red, and magenta without
/// passing through green.
///
/// Ported from https://blog.saikoled.com/post/43693602826/why-every-led-light-should-be-using-hsi
pub fn hsi_to_rgb(hue: Phase, sat: UnipolarFloat, intensity: UnipolarFloat) -> ColorRgb {
    let hue = hue + 1. / 3.;
    let (rv, gv, bv) = if hue.val() < 1. / 3. {
        let hue_rad = 2. * PI * hue.val();
        (
            (1. + sat.val() * hue_rad.cos() / (PI / 3. - hue_rad).cos()),
            (1. + sat.val() * (1. - hue_rad.cos() / (PI / 3. - hue_rad).cos())),
            (1. - sat.val()),
        )
    } else if hue.val() < 2. / 3. {
        let hue_rad = 2. * PI * (hue.val() - 1. / 3.);
        (
            (1. - sat.val()),
            (1. + sat.val() * hue_rad.cos() / (PI / 3. - hue_rad).cos()),
            (1. + sat.val() * (1. - hue_rad.cos() / (PI / 3. - hue_rad).cos())),
        )
    } else {
        let hue_rad = 2. * PI * (hue.val() - 2. / 3.);
        (
            (1. + sat.val() * (1. - hue_rad.cos() / (PI / 3. - hue_rad).cos())),
            (1. - sat.val()),
            (1. + sat.val() * hue_rad.cos() / (PI / 3. - hue_rad).cos()),
        )
    };
    let i_scale = intensity.val() / 3.0;
    [
        unit_to_u8(i_scale * rv),
        unit_to_u8(i_scale * gv),
        unit_to_u8(i_scale * bv),
    ]
}

/// Convert unit-scaled HSI into a 32-bit RGBW color.
///
/// NOTE: we shift the hue coordinate by 1/3, to put green at zero instead of red.
/// This makes it easy to turn a knob between yellow, red, and magenta without
/// passing through green.
///
/// This implementation ensures that no more than two out of three color diodes
/// are ever on at a time, which produces much nicer results in fixtures with
/// poor or absent color mixing.
///
/// Ported from https://blog.saikoled.com/post/44677718712/how-to-convert-from-hsi-to-rgb-white
pub fn hsi_to_rgbw(hue: Phase, sat: UnipolarFloat, intensity: UnipolarFloat) -> ColorRgbw {
    let hue = hue + 1. / 3.;
    let (rv, gv, bv) = if hue.val() < 1. / 3. {
        let hue_rad = 2. * PI * hue.val();
        let cos_h = hue_rad.cos();
        let cos_1047_h = (PI / 3. - hue_rad).cos();
        (
            (1. + cos_h / cos_1047_h),
            (1. + (1. - cos_h / cos_1047_h)),
            0.,
        )
    } else if hue.val() < 2. / 3. {
        let hue_rad = 2. * PI * (hue.val() - 1. / 3.);
        let cos_h = hue_rad.cos();
        let cos_1047_h = (PI / 3. - hue_rad).cos();
        (
            0.,
            (1. + cos_h / cos_1047_h),
            (1. + (1. - cos_h / cos_1047_h)),
        )
    } else {
        let hue_rad = 2. * PI * (hue.val() - 2. / 3.);
        let cos_h = hue_rad.cos();
        let cos_1047_h = (PI / 3. - hue_rad).cos();
        (
            (1. + (1. - cos_h / cos_1047_h)),
            0.,
            (1. + cos_h / cos_1047_h),
        )
    };
    let i_scale = sat.val() * intensity.val() / 3.0;
    [
        unit_to_u8(i_scale * rv),
        unit_to_u8(i_scale * gv),
        unit_to_u8(i_scale * bv),
        unit_to_u8((1. - sat.val()) * intensity.val()),
    ]
}

fn unit_to_u8(v: f64) -> u8 {
    (255. * v).round() as u8
}

/// Print a brick of color to stdout.
///
/// This can be used for debugging color output.
#[allow(unused)]
pub fn print_color([r, g, b]: ColorRgb) {
    print!("{}", "▮".truecolor(r, g, b).on_truecolor(r, g, b));
}
