//! Control profile for a dimmer.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 1]
#[strobe(Long)]
pub struct Dimmer {
    #[channel_control]
    #[animate]
    level: ChannelLevelUnipolar<UnipolarChannel>,
}

impl Default for Dimmer {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0)
                // TODO: if we need to use a dimmer channel for something
                // besides conventionals, make this configurable.
                .strobed()
                .with_channel_level(),
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

mod relay {
    //! A profile for a relay that can be turned on or off.
    use super::*;
    #[derive(Debug, EmitState, Control, Update, PatchFixture)]
    #[channel_count = 1]
    #[strobe(Short)]
    pub struct Relay {
        #[channel_control]
        level: ChannelLevelBool<BoolChannel>,
    }

    impl Default for Relay {
        fn default() -> Self {
            Self {
                level: Bool::full_channel("On", 0).strobed().with_channel_level(),
            }
        }
    }

    impl NonAnimatedFixture for Relay {
        fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
            self.level
                .render(group_controls, std::iter::empty(), dmx_buf);
        }
    }
}
