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
    /// HSLuv perceptually uniform color space, green shifted to hue = 0.
    /// www.hsluv.org
    ///
    /// The hue axis is remapped so each hue value matches the corresponding
    /// hue in HSV and HSI, keeping a given hue consistent across all three
    /// spaces.
    #[default]
    Hsluv,
    /// HSI color space with green shifted to hue = 0.
    Hsi,
    /// HSV color space with green shifted to hue = 0.
    Hsv,
}

/// Render a specific color out into various integer-based output spaces.
pub trait RenderColor {
    fn rgb(&self) -> ColorRgb;
    fn rgbw(&self) -> ColorRgbw;
    fn hsv(&self) -> ColorHsv;
    /// The (gamma-encoded) sRGB value, i.e. [`rgb`](Self::rgb) before 8-bit
    /// quantization — for consumers that need the full precision.
    fn rgb_float(&self) -> [UnipolarFloat; 3];
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
    fn rgb_float(&self) -> [UnipolarFloat; 3] {
        let (r, g, b) = self.linear_rgb();
        [
            UnipolarFloat::new(r),
            UnipolarFloat::new(g),
            UnipolarFloat::new(b),
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
    fn rgb_float(&self) -> [UnipolarFloat; 3] {
        let (r, g, b) = hsi_to_linear_rgb(self.hue, self.sat, self.intensity);
        [
            UnipolarFloat::new(r),
            UnipolarFloat::new(g),
            UnipolarFloat::new(b),
        ]
    }
}

/// A color in the HSLuv color space, with green at hue = 0.
///
/// Unlike standard HSLuv, the hue axis is remapped so each hue value matches
/// the corresponding hue in HSV and HSI; saturation and lightness keep their
/// HSLuv meaning.
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

    fn rgb_float(&self) -> [UnipolarFloat; 3] {
        let (r, g, b) = self.linear_rgb();
        [
            UnipolarFloat::new(r),
            UnipolarFloat::new(g),
            UnipolarFloat::new(b),
        ]
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

pub use cmy::*;

pub mod cmy {
    //! # Rendering a color onto a subtractive CMY moving head (dimmer + flags)
    //!
    //! A CMY head is a fixed white arc lamp → a **dimmer** (attenuates total flux,
    //! roughly spectrally flat) → three **dichroic flags** that each subtract one
    //! primary (Cyan−red, Magenta−green, Yellow−blue), progressively inserted. In
    //! linear light the emitted color is `dimmer · (1−c, 1−m, 1−y)`.
    //!
    //! [`rgb_to_cmy_dimmer`] reproduces a target RGB color by decomposing it:
    //! **`dimmer = max(r, g, b)`** and **`flags = 1 − rgb/max`** (the max-normalized
    //! chromaticity, through a [`ChromaToCmy`] model), so `dimmer · (1 − flags) = rgb`
    //! — the beam *is* the target color. Two properties fall out:
    //!
    //! - **At most two of the three flags ever insert**: the max channel's flag is
    //!   `0` (fully open), the maximum-throughput two-filter scheme.
    //! - **Chromaticity lands in the flags and brightness in the dimmer, per hue,
    //!   automatically.** The dimmer is the max channel, so at equal perceived
    //!   lightness a saturated red (one dominant channel, luminance-poor) takes a
    //!   high dimmer while yellow (two channels, luminance-rich) takes a low one —
    //!   the two emerge at equal brightness. The perceptual work is left to the
    //!   color space feeding this (e.g. HSLuv's lightness curve): pass the full
    //!   color at its actual level.
    //!
    //! The decomposition runs in the additive path's (gamma-encoded) sRGB space and
    //! in float, so a CMY head and an RGB fixture agree on a given color and the
    //! 16-bit output keeps its full resolution.
    //!
    //! ## Prior art
    //! - **US 11,221,125 B2, "Color control in subtractive color mixing system,"**
    //!   J. Gadegaard / Harman Professional Denmark (maker of the MAC 700): defines
    //!   the target in **CIE 1931 (x,y)**, treats the filter-setpoint→emitted-color
    //!   relationship as **non-linear** and **calibrated by measurement**
    //!   (spectrometer + integrating sphere), builds a **point-set mesh**
    //!   (triangulation, or a two-filter quadrilateral with an analytical bilinear
    //!   solve) of measured points, and renders a target by locating its mesh cell
    //!   and **interpolating** to filter setpoints. Only **two of three filters**
    //!   insert for any chromaticity — the invariant the max-normalization reproduces.
    //! - **ETC Eos / grandMA3**: CIE-xy color engines with per-fixture measured gamut
    //!   calibration.
    //! - **QLC+** (open source): the naive `cmy = 255 − rgb`, the floor this improves on.
    //! - **HARMAN, "additive vs subtractive"**: saturated CMY loses lumens (more
    //!   wavelengths filtered), so flags stay maximally open and luminance rides the
    //!   dimmer.
    //!
    //! ## Fidelity level and calibrated extension
    //! [`AnalyticalCmy`] is the analytical, tune-by-eye model — the patent's
    //! *theoretical* two-filter form. Its approximations: real dichroic insertion is
    //! non-linear with cross-talk; chromaticity is computed in the additive path's
    //! gamma-encoded **sRGB** space (so RGB and CMY fixtures agree on an HSLuv color)
    //! rather than the fixture's actual CMY primaries (so hues are approximate); and
    //! the dimmer→light and flag-DMX→attenuation curves are treated as linear.
    //!
    //! A calibrated model implements the same [`ChromaToCmy`] trait, backed by a
    //! measured DMX→CIE-xy table for the head (spectrometer + integrating sphere per
    //! the patent, or published data): a CIE-xy gamut mesh with inverse interpolation
    //! target → flag setpoints, gamut mapping of out-of-range targets toward the
    //! boundary (with the color wheel / CTC slots for extremes), and a measured
    //! dimmer response curve for perceptual luminance → dimmer.

    use number::UnipolarFloat;

    /// The drive for a subtractive CMY + dimmer head: three flag fractions plus
    /// an overall dimmer level, as an intermediate representation independent of
    /// how those four values are rendered onto DMX channels.
    #[derive(Debug, Clone, Copy)]
    pub struct CmyDimmer {
        pub cyan: UnipolarFloat,
        pub magenta: UnipolarFloat,
        pub yellow: UnipolarFloat,
        pub dimmer: UnipolarFloat,
    }

    /// Map a chromaticity (max-normalized RGB, max component == 1) to CMY
    /// subtractive flag fractions `[c, m, y]` (`0` = flag open / no subtraction,
    /// `1` = flag fully inserted).
    pub trait ChromaToCmy {
        fn flags(&self, chroma_rgb: [UnipolarFloat; 3]) -> [UnipolarFloat; 3];
    }

    /// Analytical CMY model: `flag = 1 − chroma`. With a normalized chromaticity
    /// (max component `1`), exactly one flag is `0`, so at most two flags insert.
    #[derive(Debug, Clone, Copy)]
    pub struct AnalyticalCmy;

    impl ChromaToCmy for AnalyticalCmy {
        fn flags(&self, chroma_rgb: [UnipolarFloat; 3]) -> [UnipolarFloat; 3] {
            chroma_rgb.map(|c| c.invert())
        }
    }

    /// Decompose a float sRGB color into a CMY + dimmer drive that reproduces it:
    /// the dimmer is the max channel, and the flags come from `model` applied to
    /// the max-normalized chromaticity, so `dimmer · (1 − flags) = rgb`. A black
    /// input yields a zero dimmer with fully open flags.
    pub fn rgb_to_cmy_dimmer(rgb: [UnipolarFloat; 3], model: &impl ChromaToCmy) -> CmyDimmer {
        let [r, g, b] = rgb.map(|c| c.val());
        let m = r.max(g).max(b);
        // Normalize by the brightest channel to separate chromaticity from
        // brightness. Each channel is <= m, so the ratios already land in [0, 1];
        // only true black (m == 0) is undefined, and there every flag opens.
        let chroma = if m > 0.0 {
            [
                UnipolarFloat::new(r / m),
                UnipolarFloat::new(g / m),
                UnipolarFloat::new(b / m),
            ]
        } else {
            [UnipolarFloat::ONE; 3]
        };
        let [cyan, magenta, yellow] = model.flags(chroma);
        CmyDimmer {
            cyan,
            magenta,
            yellow,
            dimmer: UnipolarFloat::new(m),
        }
    }
}

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
    let (r, g, b) = hsi_to_linear_rgb(hue, sat, intensity);
    [unit_to_u8(r), unit_to_u8(g), unit_to_u8(b)]
}

fn hsi_to_linear_rgb(hue: Phase, sat: UnipolarFloat, intensity: UnipolarFloat) -> (f64, f64, f64) {
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
    (i_scale * rv, i_scale * gv, i_scale * bv)
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

#[cfg(test)]
mod cmy_tests {
    use number::{Phase, UnipolarFloat};

    use super::cmy::*;

    fn uf(v: f64) -> UnipolarFloat {
        UnipolarFloat::new(v)
    }

    fn flags_of(c: &CmyDimmer) -> [f64; 3] {
        [c.cyan.val(), c.magenta.val(), c.yellow.val()]
    }

    #[test]
    fn analytical_flags_are_inverse_chroma() {
        // `flag = 1 - chroma`: subtract the complementary channel.
        let f = |c: [f64; 3]| AnalyticalCmy.flags(c.map(uf)).map(|x| x.val());
        assert_eq!(f([1.0, 1.0, 1.0]), [0.0, 0.0, 0.0]); // white → all open
        assert_eq!(f([1.0, 0.0, 0.0]), [0.0, 1.0, 1.0]); // red → cyan open
        assert_eq!(f([0.0, 1.0, 0.0]), [1.0, 0.0, 1.0]); // green → magenta open
        assert_eq!(f([0.0, 0.0, 1.0]), [1.0, 1.0, 0.0]); // blue → yellow open
    }

    #[test]
    fn decomposition_reproduces_color_and_separates_hue_from_brightness() {
        let cmy = |rgb: [f64; 3]| rgb_to_cmy_dimmer(rgb.map(uf), &AnalyticalCmy);
        for rgb in [
            [0.8, 0.3, 0.1],
            [0.1, 0.9, 0.5],
            [1.0, 1.0, 0.0],
            [0.4, 0.0, 0.0],
        ] {
            let c = cmy(rgb);
            let d = c.dimmer.val();
            let flags = flags_of(&c);
            // dimmer · (1 - flags) reproduces the target color.
            for i in 0..3 {
                assert!(
                    (d * (1.0 - flags[i]) - rgb[i]).abs() < 1e-9,
                    "reproduce {rgb:?}"
                );
            }
            // dimmer is the max channel; at most two flags insert.
            assert!((d - rgb.iter().copied().fold(0.0, f64::max)).abs() < 1e-9);
            assert!(flags.iter().any(|f| *f < 1e-9), "one flag open: {flags:?}");
        }
        // Same hue at different brightness → identical flags; dimmer tracks brightness.
        let full = cmy([1.0, 0.0, 0.0]);
        let dim = cmy([0.4, 0.0, 0.0]);
        assert_eq!(flags_of(&full), flags_of(&dim));
        assert!((full.dimmer.val() - 1.0).abs() < 1e-9);
        assert!((dim.dimmer.val() - 0.4).abs() < 1e-9);
    }

    #[test]
    fn decomposition_edge_cases() {
        let white = rgb_to_cmy_dimmer([0.6, 0.6, 0.6].map(uf), &AnalyticalCmy);
        assert_eq!(flags_of(&white), [0.0, 0.0, 0.0]); // all flags open
        assert!((white.dimmer.val() - 0.6).abs() < 1e-9);
        let black = rgb_to_cmy_dimmer([0.0, 0.0, 0.0].map(uf), &AnalyticalCmy);
        assert_eq!(black.dimmer.val(), 0.0); // dark
    }

    #[test]
    fn open_flag_tracks_dominant_channel() {
        // End-to-end through the HSLuv pipeline: the flag that subtracts the
        // dominant color channel is fully open (cyan↔red, magenta↔green,
        // yellow↔blue). At secondary hues two channels tie and both flags open.
        use super::{Hsluv, RenderColor};
        for i in 0..12 {
            let hue = Phase::new(i as f64 / 12.0);
            let rgb = Hsluv {
                hue,
                sat: uf(1.0),
                lightness: uf(0.3225),
            }
            .rgb_float();
            let flags = flags_of(&rgb_to_cmy_dimmer(rgb, &AnalyticalCmy));
            let max_ch = (0..3)
                .max_by(|&a, &b| rgb[a].val().total_cmp(&rgb[b].val()))
                .unwrap();
            assert!(
                flags[max_ch] < 1e-5,
                "hue {}: the dominant channel {max_ch}'s flag should be open; rgb={rgb:?} flags={flags:?}",
                hue.val()
            );
        }
    }
}
