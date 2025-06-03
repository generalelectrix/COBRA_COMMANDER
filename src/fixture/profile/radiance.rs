//! Control profile for a Radiance hazer.
//! Probably fine for any generic 2-channel hazer.
use anyhow::Result;
use std::{collections::HashMap, time::Duration};

use crate::fixture::prelude::*;

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

impl PatchAnimatedFixture for Radiance {
    const NAME: FixtureType = FixtureType("Radiance");
    fn channel_count(&self, _render_mode: Option<RenderMode>) -> usize {
        2
    }

    fn new(options: &HashMap<String, String>) -> Result<(Self, Option<RenderMode>)> {
        let mut s = Self::default();
        if options.contains_key("use_timer") {
            s.timer = Some(Timer::from_options(options)?);
        }
        Ok((s, None))
    }
}

crate::register!(Radiance);

impl AnimatedFixture for Radiance {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
        _animation_vals: TargetedAnimationValues<Self::Target>,
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

impl ControllableFixture for Radiance {
    fn update(&mut self, _: &MasterControls, delta_t: Duration) {
        if let Some(timer) = self.timer.as_mut() {
            timer.update(delta_t);
        }
    }
}
