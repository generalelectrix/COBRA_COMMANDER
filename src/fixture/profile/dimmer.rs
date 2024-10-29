//! Control profile for a dimmer.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control)]
pub struct Dimmer {
    #[channel_control]
    #[animate]
    level: UnipolarChannelLevel<UnipolarChannel>,
}

impl Default for Dimmer {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0).with_channel_level(),
        }
    }
}

impl PatchAnimatedFixture for Dimmer {
    const NAME: FixtureType = FixtureType("Dimmer");
    fn channel_count(&self) -> usize {
        1
    }
}

impl AnimatedFixture for Dimmer {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.level.render(animation_vals.all(), dmx_buf);
    }
}

impl ControllableFixture for Dimmer {}
