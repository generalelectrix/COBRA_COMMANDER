//! Things related to color spaces, color models, rendering colors, etc.

use std::f64::consts::PI;

use fixture_macros::AsPatchOption;
use hsluv::hsluv_to_rgb;
use log::debug;
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
        linear_rgb_to_rgbw(self.linear_rgb())
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

impl Hsv {
    /// HSV to linear sRGB in `[0, 1]`.
    fn linear_rgb(&self) -> (f64, f64, f64) {
        hsv_to_linear_rgb(self.hue, self.sat, self.val)
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
        let (r, g, b) = self.linear_rgb();
        [unit_to_u8(r), unit_to_u8(g), unit_to_u8(b)]
    }

    fn rgbw(&self) -> ColorRgbw {
        linear_rgb_to_rgbw(self.linear_rgb())
    }

    fn hsv(&self) -> ColorHsv {
        debug!("HSV output rendering is not implemented for HSLuv");
        [0, 0, 0]
    }
}

impl Hsluv {
    /// HSLuv to linear sRGB in `[0, 1]`. The hue coordinate is mapped via a
    /// 24-anchor Catmull-Rom approximation of
    ///
    ///   f(u) = HSLuv_hue( HSV(u, sat=1, val=1) )
    ///
    /// so that named hues land at their HSV positions and the within-
    /// segment sweep follows HSV's RGB-uniform shape rather than CIELUV's
    /// perceptual-uniformity expansion of green and yellow. Saturation and
    /// lightness are interpreted as HSLuv values. Out-of-gamut channels are
    /// clipped to the displayable range.
    fn linear_rgb(&self) -> (f64, f64, f64) {
        // The exact `f` above asks "what HSLuv hue produces the same
        // chromaticity as HSV at this input?" Calling its closed form
        // requires two hsluv-crate round-trips per render; the spline
        // below approximates `f` directly so the runtime cost is one
        // hsluv_to_rgb call plus a handful of multiplies.
        //
        // `f` has surprising shape: it's nearly flat at the three RGB
        // primaries (tangent ≈ 2°/unit-t) because pure-primary HSV stays
        // near its HSLuv counterpart under small chromatic perturbations,
        // and very steep between primary and secondary (tangent
        // approaching 40°/unit-t through cyan and yellow) where the
        // HSV→RGB mix moves the chromaticity quickly around the wheel.
        // 24 anchors uniformly spaced in user input capture the curvature
        // to within ~0.74° max error vs the exact inverse.
        //
        // Values are unwrapped (monotonically increasing past 360°) so
        // the segment math doesn't need to handle the green↔red wrap.
        // hsluv_to_rgb accepts angles > 360° and wraps internally.
        const N_SEG: usize = 24;
        const ANCHOR_DEG: [f64; N_SEG] = [
            127.7150, 129.6099, 136.6877, 155.0453, 192.1771, 233.5490, 255.5095, 263.7201,
            265.8743, 267.9638, 274.9051, 288.4547, 307.7150, 331.9367, 355.3484, 368.3239,
            372.1771, 376.0787, 389.9437, 417.2823, 445.8743, 466.4839, 479.5531, 485.8594,
        ];
        // Catmull-Rom tangents (df/dt with t ∈ [0, 1] spanning one segment).
        const ANCHOR_TAN: [f64; N_SEG] = [
            1.8752, 4.4864, 12.7177, 27.7447, 39.2519, 31.6662, 15.0855, 5.1824, 2.1219, 4.5154,
            10.2454, 16.4049, 21.7410, 23.8167, 18.1936, 8.4143, 3.8774, 8.8833, 20.6018, 27.9653,
            24.6008, 16.8394, 9.6877, 4.0810,
        ];

        let scaled = self.hue.val() * N_SEG as f64;
        let seg = (scaled as usize).min(N_SEG - 1);
        let t = scaled - seg as f64;
        let p0 = ANCHOR_DEG[seg];
        let m0 = ANCHOR_TAN[seg];
        let (p1, m1) = if seg + 1 == N_SEG {
            // Wraparound: anchor 0 of the next period sits at f[0] + 360°
            // with the same tangent (function is periodic).
            (ANCHOR_DEG[0] + 360.0, ANCHOR_TAN[0])
        } else {
            (ANCHOR_DEG[seg + 1], ANCHOR_TAN[seg + 1])
        };
        let t2 = t * t;
        let t3 = t2 * t;
        let hue_degrees = (2.0 * t3 - 3.0 * t2 + 1.0) * p0
            + (t3 - 2.0 * t2 + t) * m0
            + (-2.0 * t3 + 3.0 * t2) * p1
            + (t3 - t2) * m1;
        let (r, g, b) = hsluv_to_rgb(
            hue_degrees,
            self.sat.val() * 100.,
            self.lightness.val() * 100.,
        );
        (r.clamp(0., 1.), g.clamp(0., 1.), b.clamp(0., 1.))
    }
}

/// This is the lightness value where the gamut contains all three primary colors
/// at the brightness equivalent to blue at maximum output.
pub const HSLUV_LIGHTNESS_OFFSET: UnipolarFloat = UnipolarFloat::new(0.3225);

/// W diode luminous output relative to a single chromatic LED, expressed in
/// chromatic-channel-units.
///
/// Typical entertainment-grade RGBW pars (Cree XM-L Color, Luxeon C Color)
/// and six-die hex pars (Osram OSTAR Stage) sit near 2.0. A value of 3.0
/// corresponds to the idealized "W = R+G+B equal-mix" regime; 1.0
/// corresponds to the saikoled HSI→RGBW regime, appropriate for
/// SK6812-class pixel strings and warm-white phosphor parts.
pub const W_DIODE_BRIGHTNESS: f64 = 2.0;

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
    let (r, g, b) = hsv_to_linear_rgb(hue, sat, val);
    [unit_to_u8(r), unit_to_u8(g), unit_to_u8(b)]
}

fn hsv_to_linear_rgb(hue: Phase, sat: UnipolarFloat, val: UnipolarFloat) -> (f64, f64, f64) {
    let hue = hue + 1. / 3.;
    if sat == 0.0 {
        return (val.val(), val.val(), val.val());
    }
    let var_h = if hue == 1.0 { 0.0 } else { hue.val() * 6.0 };

    let var_i = var_h.floor();
    let var_1 = val.val() * (1.0 - sat.val());
    let var_2 = val.val() * (1.0 - sat.val() * (var_h - var_i));
    let var_3 = val.val() * (1.0 - sat.val() * (1.0 - (var_h - var_i)));

    match var_i as i64 {
        0 => (val.val(), var_3, var_1),
        1 => (var_2, val.val(), var_1),
        2 => (var_1, val.val(), var_3),
        3 => (var_1, var_2, val.val()),
        4 => (var_3, var_1, val.val()),
        _ => (val.val(), var_1, var_2),
    }
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

/// Convert linear RGB in `[0, 1]` to an RGBW drive vector via
/// brightness-aware white subtraction.
///
/// The achromatic minimum of the input is migrated to the W channel, scaled
/// by [`W_DIODE_BRIGHTNESS`] to compensate for the W diode's lumen output
/// relative to a single chromatic diode. On a fixture whose W actually
/// emits `W_DIODE_BRIGHTNESS` chromatic-channel-units of light, the
/// four-channel output reproduces the spectrum the chromatic channels
/// would emit alone, so chromaticity and brightness are preserved.
///
/// Exactly one of R/G/B is zero except on near-white inputs where W
/// saturates at unit drive, in which case the chromatic channels carry
/// the residual achromatic load.
fn linear_rgb_to_rgbw((r, g, b): (f64, f64, f64)) -> ColorRgbw {
    let m = r.min(g).min(b);
    let w = (3. * m / W_DIODE_BRIGHTNESS).min(1.);
    let c = w * W_DIODE_BRIGHTNESS / 3.;
    [
        unit_to_u8(r - c),
        unit_to_u8(g - c),
        unit_to_u8(b - c),
        unit_to_u8(w),
    ]
}

fn unit_to_u8(v: f64) -> u8 {
    (255. * v).round() as u8
}
