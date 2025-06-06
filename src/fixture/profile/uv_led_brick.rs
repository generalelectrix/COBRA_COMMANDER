//! Control profile for a uv_led_brick.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 7]
pub struct UvLedBrick {
    #[channel_control]
    #[animate]
    level: ChannelLevelUnipolar<UnipolarChannel>,
}

impl Default for UvLedBrick {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0).with_channel_level(),
        }
    }
}

impl AnimatedFixture for UvLedBrick {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.level.render(animation_vals.all(), dmx_buf);
        dmx_buf[4] = 255;
        dmx_buf[5] = 255;
        dmx_buf[6] = 255;
    }
}

impl ControllableFixture for UvLedBrick {}
