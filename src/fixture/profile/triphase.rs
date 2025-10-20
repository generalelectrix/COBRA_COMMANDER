//! The original swarm of beams... the good ol' American DJ TriPhase.
//!
//! If only they'd just given us color control...

use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 4]
#[strobe]
pub struct TriPhase {
    red: Bool<()>,
    green: Bool<()>,
    blue: Bool<()>,

    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,

    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for TriPhase {
    fn default() -> Self {
        Self {
            red: Bool::new_off("Red", ()),
            green: Bool::new_off("Green", ()),
            blue: Bool::new_off("Blue", ()),
            dimmer: Unipolar::full_channel("Dimmer", 3)
                .strobed_short()
                .with_channel_level(),
            // strobe: Strobe::channel("Strobe", 2, 1, 255, 0),
            rotation: Bipolar::split_channel("Rotation", 1, 120, 10, 135, 245, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
        }
    }
}

impl AnimatedFixture for TriPhase {
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
        dmx_buf[2] = 0; // strobe
        self.rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );

        dmx_buf[0] = 32
            * match (self.red.val(), self.green.val(), self.blue.val()) {
                (false, false, false) => {
                    // belt and suspenders - dimmer should be off
                    dmx_buf[3] = 0;
                    0
                }
                (true, false, false) => 1,
                (false, true, false) => 2,
                (false, false, true) => 3,
                (true, true, false) => 4,
                (true, false, true) => 5,
                (false, true, true) => 6,
                (true, true, true) => 7,
            };
    }
}
