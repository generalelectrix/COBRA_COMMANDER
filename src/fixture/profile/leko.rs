//! Control a single leko with a gobo rotator, most flexibly a DHA Varispeed.
//!
//! NOTE: we do not have a clean way to specify that a single fixture requires
//! more than one distinct DMX buffer range to render into. We currently hack
//! our way around this by patching two instances of Leko per fixture, one with
//! the dimmer model, and the other with the rotator model. This essentially
//! works, though it does imply that the animation phase of the dimmers will be
//! slightly offset from the animation phase of the rotators.  This *probably*
//! doesn't matter in practice, and it will especially not matter if we're using
//! random noise generation to drive them.
use std::str::FromStr;

use anyhow::Context;
use log::error;
use strum_macros::{EnumString, VariantArray};

use crate::fixture::{fixture::EnumRenderModel, prelude::*};

#[derive(Debug, EmitState, Control)]
pub struct Leko {
    #[channel_control]
    #[animate]
    level: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    gobo1: ChannelKnobBipolar<Bipolar<()>>,
    #[channel_control]
    #[animate]
    gobo2: ChannelKnobBipolar<Bipolar<()>>,
}

impl Default for Leko {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0).with_channel_level(),
            gobo1: Bipolar::new("Gobo1", ()).with_detent().with_channel_knob(0),
            gobo2: Bipolar::new("Gobo2", ()).with_detent().with_channel_knob(1),
        }
    }
}

impl PatchAnimatedFixture for Leko {
    const NAME: FixtureType = FixtureType("Leko");
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize {
        Model::model_for_mode(render_mode).unwrap().channel_count()
    }

    fn new(options: &mut crate::config::Options) -> anyhow::Result<(Self, Option<RenderMode>)> {
        let Some(kind) = options.remove("kind") else {
            bail!("missing required option: kind");
        };
        let model =
            Model::from_str(&kind).with_context(|| format!("invalid kind option: {kind}"))?;
        Ok((Default::default(), Some(model.render_mode())))
    }
}

register_patcher!(Leko);

impl AnimatedFixture for Leko {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let model = match Model::model_for_mode(group_controls.render_mode) {
            Ok(m) => m,
            Err(err) => {
                error!("failed to render Leko: {err}");
                return;
            }
        };
        match model {
            Model::Dimmer => {
                self.level
                    .render(animation_vals.filter(&AnimationTarget::Level), dmx_buf);
            }
            Model::GoboSpinnaz => {
                render_gobo_spinna(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    &mut dmx_buf[0..2],
                );
                render_gobo_spinna(
                    self.gobo2
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo2)),
                    &mut dmx_buf[2..4],
                );
            }
            Model::DhaVarispeed => {
                render_varispeed(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    &mut dmx_buf[0..2],
                );
                render_varispeed(
                    self.gobo2
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo2)),
                    &mut dmx_buf[2..4],
                );
            }
            Model::DcRotator => {
                let animated = self
                    .gobo1
                    .control
                    .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1));
                dmx_buf[0] = unipolar_to_range(0, 255, animated.abs());
            }
        }
    }
}

impl ControllableFixture for Leko {}

/// Which model of gobo rotator is installed in this leko, or is this the dimmer.
#[derive(Default, Debug, Clone, Copy, Eq, PartialEq, EnumString, VariantArray)]
enum Model {
    /// Dimmer channel.
    #[default]
    Dimmer,
    /// DHA Varispeed controlled using the GOBO SPINNAZ module.
    GoboSpinnaz,
    /// DHA Varispeed controlled using the DHA DMX DC controller.
    DhaVarispeed,
    /// Dimmer channel controlling a DC wall wart, such as TwinSpin OG or film FX.
    DcRotator,
}

impl EnumRenderModel for Model {}

impl Model {
    fn channel_count(&self) -> usize {
        match self {
            Self::Dimmer => 1,
            Self::GoboSpinnaz => 4,
            Self::DhaVarispeed => 4,
            Self::DcRotator => 1,
        }
    }
}

// See notes in color_huster.gobo_rotator for why these are the way they are.

fn render_gobo_spinna(val: BipolarFloat, dmx_buf: &mut [u8]) {
    // direction
    dmx_buf[0] = if val <= BipolarFloat::ZERO { 0 } else { 255 };
    // speed
    dmx_buf[1] = unipolar_to_range(0, 255, val.abs());
}

fn render_varispeed(val: BipolarFloat, dmx_buf: &mut [u8]) {
    // varispeed at DMX 250 is very close to GOBO SPINNA at DMX 255,
    // varispeed has a small detent such that DMX value 5 isn't spinning at all.
    let mut speed_int = (val.val().abs() * 245.) as u8;
    if speed_int > 0 {
        // If we're spinning at all, start from DMX value 6, and range up to 250.
        speed_int += 5;
    }
    if val < BipolarFloat::ZERO {
        dmx_buf[0] = speed_int;
        dmx_buf[1] = 0;
    } else {
        dmx_buf[0] = 0;
        dmx_buf[1] = speed_int;
    }
}
