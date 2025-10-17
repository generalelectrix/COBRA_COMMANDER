//! Control profile for a dimmer.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 1]
pub struct Dimmer {
    #[channel_control]
    #[animate]
    level: ChannelLevelUnipolar<UnipolarChannel>,
}

impl Default for Dimmer {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0).with_channel_level(),
        }
    }
}

impl AnimatedFixture for Dimmer {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.level
            .render(group_controls, animation_vals.all(), dmx_buf);
    }
}
