//! Flexible control profile for a single-color fixture.
use anyhow::{Context, Result};
use log::error;
use strum_macros::{Display, EnumIter, EnumString, VariantArray};

use crate::{color::*, config::Options, fixture::prelude::*};

#[derive(Debug, Control, EmitState, Update)]
#[strobe]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupOptions {
    control_color_space: ColorSpace,
}

impl PatchFixture for Color {
    const NAME: FixtureType = FixtureType("Color");

    fn new(options: Options) -> Result<Self> {
        let options: GroupOptions = options.parse()?;
        Ok(Self::for_subcontrol(None, options.control_color_space))
    }

    fn group_options() -> Vec<(String, PatchOption)> {
        vec![(
            "control_color_space".to_string(),
            ColorSpace::patch_option(),
        )]
    }

    fn patch_options() -> Vec<(String, PatchOption)> {
        vec![("kind".to_string(), Model::patch_option())]
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchOptions {
    kind: Model,
}

impl CreatePatchConfig for Color {
    fn patch(&self, options: Options) -> Result<PatchConfig> {
        let options: PatchOptions = options.parse()?;
        Ok(PatchConfig {
            channel_count: options.kind.channel_count(),
            render_mode: Some(options.kind.render_mode()),
        })
    }
}

register_patcher!(Color);

impl Color {
    /// Construct a color whose OSC controls are optionally suffixed.
    pub fn for_subcontrol(control_suffix: Option<usize>, space: ColorSpace) -> Self {
        let suffixed = |control: &str| {
            let Some(suffix) = control_suffix else {
                return control.to_string();
            };
            format!("{control}{suffix}")
        };

        Self {
            hue: PhaseControl::new(suffixed("Hue"), ())
                .at_half()
                .with_channel_knob(0),
            sat: Unipolar::new(suffixed("Sat"), ())
                .at_full()
                .with_channel_knob(1),
            val: Unipolar::new(suffixed("Val"), ()).with_channel_level(),
            lightness_boost: (space == ColorSpace::Hsluv)
                .then_some(Unipolar::new(suffixed("LightnessBoost"), ()).with_channel_knob(2)),
            space,
        }
    }

    /// Return a lightness value for HSLuv.
    /// Return 0 if we unexpectedly don't have a lightness boost control configured.
    /// This does NOT include the rescaling from the overall level fader.
    fn hsluv_lightness(&self) -> UnipolarFloat {
        let Some(lightness_boost) = &self.lightness_boost else {
            error!("No lightness boost control configured for HSLuv color.");
            return UnipolarFloat::ZERO;
        };

        HSLUV_LIGHTNESS_OFFSET + (HSLUV_LIGHTNESS_OFFSET.invert()) * lightness_boost.control.val()
    }

    pub fn render_without_animations(&self, model: Model, dmx_buf: &mut [u8]) {
        match self.space {
            ColorSpace::Hsv => model.render(
                dmx_buf,
                Hsv {
                    hue: self.hue.control.val(),
                    sat: self.sat.control.val(),
                    val: self.val.control.val(),
                },
            ),
            ColorSpace::Hsi => model.render(
                dmx_buf,
                Hsi {
                    hue: self.hue.control.val(),
                    sat: self.sat.control.val(),
                    intensity: self.val.control.val(),
                },
            ),
            ColorSpace::Hsluv => model.render(
                dmx_buf,
                Hsluv {
                    hue: self.hue.control.val(),
                    sat: self.sat.control.val(),
                    lightness: self.hsluv_lightness() * self.val.control.val(),
                },
            ),
        }
    }

    /// Render this color into a DMX output buffer with an explicit color model.
    ///
    /// This method is useful for fixtures that embed a Color as a full sub-
    /// control.
    pub fn render_for_model(
        &self,
        model: Model,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<AnimationTarget>,
        dmx_buf: &mut [u8],
    ) {
        // If a color override has been provided, render it scaled by the level.
        if let Some(mut color_override) = group_controls.color.clone() {
            // TODO: do we want to allow strobing to layer on top of a color override?
            color_override.lightness *= self.val.control.val();
            model.render(dmx_buf, color_override);
            return;
        }

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

        if let Some(strobe_intensity) =
            group_controls.strobe_intensity(crate::strobe::StrobeResponse::Short)
        {
            val = strobe_intensity.val();
        }

        match self.space {
            ColorSpace::Hsv => model.render(
                dmx_buf,
                Hsv {
                    hue: Phase::new(hue),
                    sat: UnipolarFloat::new(sat),
                    val: UnipolarFloat::new(val),
                },
            ),
            ColorSpace::Hsi => model.render(
                dmx_buf,
                Hsi {
                    hue: Phase::new(hue),
                    sat: UnipolarFloat::new(sat),
                    intensity: UnipolarFloat::new(val),
                },
            ),
            ColorSpace::Hsluv => model.render(
                dmx_buf,
                Hsluv {
                    hue: Phase::new(hue),
                    sat: UnipolarFloat::new(sat),
                    lightness: self.hsluv_lightness() * UnipolarFloat::new(val),
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
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let model = match Model::model_for_mode(group_controls.render_mode) {
            Ok(m) => m,
            Err(err) => {
                error!("failed to render Color: {err}");
                return;
            }
        };

        self.render_for_model(model, group_controls, animation_vals, dmx_buf);
    }
}

#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, VariantArray, Display, EnumIter,
)]
pub enum Model {
    /// RGB in 3 DMX channels.
    #[default]
    Rgb,
    /// Dimmer in first channel + RGB.
    DimmerRgb,
    /// RGBW in 4 DMX channels.
    Rgbw,
    /// Dimmer in first channel + RGBW.
    DimmerRgbw,
    /// Dimmer in first channel + RGBW plus two unused channels (common 7-channel profile).
    SevenChannelRgbw,
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
            Self::SevenChannelRgbw => 7,
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
            Self::SevenChannelRgbw => {
                Self::DimmerRgbw.render(&mut buf[0..5], renderer);
                buf[5] = 0;
                buf[6] = 0;
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
