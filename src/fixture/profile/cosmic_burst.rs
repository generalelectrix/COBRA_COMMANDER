//! Control profile for the Cosmic Burst white laser moonflower.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 6]
#[strobe_external]
pub struct CosmicBurst {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    //strobe: StrobeChannel,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}
impl Default for CosmicBurst {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 2)
                .strobed_short()
                .with_channel_level(),
            // strobe: Strobe::channel("Strobe", 1, 64, 95, 32),
            rotation: Bipolar::split_channel("Rotation", 0, 125, 8, 130, 247, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
        }
    }
}

impl AnimatedFixture for CosmicBurst {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        // self.strobe
        //     .render(group_controls, std::iter::empty(), dmx_buf);
        self.rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
    }
}
