//! The original swarm of beams... the good ol' American DJ TriPhase.
//!
//! If only they'd just given us color control...

use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 4]
pub struct TriPhase {
    red: Bool<()>,
    green: Bool<()>,
    blue: Bool<()>,
    white: Bool<()>,

    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    strobe: StrobeChannel,

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
            white: Bool::new_off("White", ()),
            dimmer: Unipolar::full_channel("Dimmer", 3).with_channel_level(),
            strobe: Strobe::channel("Strobe", 2, 1, 255, 0),
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
        self.dimmer
            .render(animation_vals.filter(&AnimationTarget::Dimmer), dmx_buf);
        self.strobe
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );

        dmx_buf[0] = match (
            self.red.val(),
            self.green.val(),
            self.blue.val(),
            self.white.val(),
        ) {
            (false, false, false, false) => {
                // All off - override the dimmer setting to 0.
                dmx_buf[3] = 0;
                0
            }
            (true, false, false, false) => 0,
            (false, true, false, false) => 17,
            (false, false, true, false) => 34,
            (false, false, false, true) => 51,
            (true, true, false, false) => 68,
            (true, false, true, false) => 85,
            (true, false, false, true) => 102,
            (false, true, true, false) => 119,
            (false, true, false, true) => 136,
            (false, false, true, true) => 153,
            (true, true, true, false) => 170,
            (true, true, false, true) => 187,
            (true, false, true, true) => 204,
            (false, true, true, true) => 221,
            (true, true, true, true) => 238,
        };
    }
}
