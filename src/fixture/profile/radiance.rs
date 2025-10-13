//! Control profile for a Radiance hazer.
//! Probably fine for any generic 2-channel hazer.
use anyhow::Result;
use std::time::Duration;

use crate::{config::Options, fixture::prelude::*};

#[derive(Debug, EmitState, Control)]
pub struct Radiance {
    #[channel_control]
    haze: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    fan: ChannelKnobUnipolar<UnipolarChannel>,
    #[skip_emit]
    #[skip_control]
    timer: Option<Timer>,
}

impl Default for Radiance {
    fn default() -> Self {
        Self {
            haze: Unipolar::full_channel("Haze", 0).with_channel_level(),
            fan: Unipolar::full_channel("Fan", 1).with_channel_knob(0),
            timer: None,
        }
    }
}

impl PatchFixture for Radiance {
    const NAME: FixtureType = FixtureType("Radiance");

    fn new(options: &mut Options) -> Result<Self> {
        let mut s = Self::default();
        if options.remove("use_timer").is_some() {
            s.timer = Some(Timer::from_options(options)?);
        }
        Ok(s)
    }

    fn patch_config(_options: &mut Options) -> Result<PatchConfig> {
        Ok(PatchConfig {
            channel_count: 2,
            render_mode: None,
        })
    }

    fn group_options() -> Vec<(String, PatchOption)> {
        vec![]
    }

    fn patch_options() -> Vec<(String, PatchOption)> {
        vec![]
    }
}

register_patcher!(Radiance);

impl AnimatedFixture for Radiance {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
        _animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        if let Some(timer) = self.timer.as_ref() {
            if !timer.is_on() {
                dmx_buf[0] = 0;
                dmx_buf[1] = 0;
                return;
            }
        }
        self.haze.render_no_anim(dmx_buf);
        self.fan.render_no_anim(dmx_buf);
    }
}

impl Update for Radiance {
    fn update(&mut self, _: &MasterControls, delta_t: Duration) {
        if let Some(timer) = self.timer.as_mut() {
            timer.update(delta_t);
        }
    }
}
