//! Control profile for the "house light" Starlight white laser moonflower.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 4]
pub struct Starlight {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    strobe: StrobeChannel,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}
impl Default for Starlight {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 1).with_channel_level(),
            strobe: Strobe::channel("Strobe", 2, 10, 255, 0),
            rotation: Bipolar::split_channel("Rotation", 3, 191, 128, 192, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
        }
    }
}

impl AnimatedFixture for Starlight {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        dmx_buf[0] = 255; // DMX mode
        self.dimmer
            .render(animation_vals.filter(&AnimationTarget::Dimmer), dmx_buf);
        self.strobe
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
    }
}

impl ControllableFixture for Starlight {}
