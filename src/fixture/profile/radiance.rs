//! Control profile for a Radiance hazer.
//! Probably fine for any generic 2-channel hazer.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 2]
pub struct Radiance {
    #[channel_control]
    #[animate]
    haze: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    fan: ChannelKnobUnipolar<UnipolarChannel>,
}

impl Default for Radiance {
    fn default() -> Self {
        Self {
            haze: Unipolar::full_channel("Haze", 0).with_channel_level(),
            fan: Unipolar::full_channel("Fan", 1).with_channel_knob(0),
        }
    }
}

impl AnimatedFixture for Radiance {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        _animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.haze
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.fan.render(group_controls, std::iter::empty(), dmx_buf);
    }
}
