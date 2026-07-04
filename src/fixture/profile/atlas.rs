//! Control profile for the Clay Paky Atlas.
//!
//! A single DMX channel drives the band aperture: at 0% the bands are fully
//! obscured, opening to a fan effect through 25%, then progressively obscuring
//! again from right to left up to 54.7%.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 1]
pub struct Atlas {
    #[channel_control]
    #[animate]
    shutter: ChannelLevelUnipolar<UnipolarChannel>,
}

impl Default for Atlas {
    fn default() -> Self {
        Self {
            // The described fan/obscure behavior occupies 0%-54.7% of the
            // channel (DMX 0-139); the remainder is macro programs we don't map.
            shutter: Unipolar::channel("Shutter", 0, 0, 139).with_channel_level(),
        }
    }
}

impl AnimatedFixture for Atlas {
    type Target = AnimationTarget;

    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>,
    {
        self.shutter
            .render(group_controls, animation_vals.all(), dmx_buf);
    }
}
