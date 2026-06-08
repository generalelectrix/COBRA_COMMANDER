//! Chauvet DJ Abyss 2 simulated-water effect light.
//!
//! Three channels: dimmer, a color wheel that doubles as bidirectional color scroll, and a
//! bidirectional wave motor. Structurally a close cousin of the American DJ H2O.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 3]
#[strobe(Short)]
pub struct Abyss2 {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    /// Discrete color-wheel selection (shares channel 2 with `color_rotation`).
    fixed_color: LabeledSelect,
    /// Scroll the color wheel instead of holding a fixed color.
    // Gates `color_rotation`, which shares the color channel.
    color_rotate: Bool<()>,
    #[channel_control]
    #[animate]
    color_rotation: ChannelKnobBipolar<BipolarSplitChannel>,
    /// Bidirectional water-wave motor.
    #[channel_control]
    #[animate]
    wave: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for Abyss2 {
    fn default() -> Self {
        Self {
            // Ch 1: dimmer 0-100%. Strobing modulates it (the fixture has no strobe channel).
            dimmer: Unipolar::full_channel("Dimmer", 0)
                .strobed()
                .with_channel_level(),
            // Ch 2 (low half): six discrete color bands, ~21 units wide; values at band centers.
            fixed_color: LabeledSelect::new(
                "FixedColor",
                1,
                vec![
                    ("White", 10),
                    ("Magenta", 32),
                    ("Yellow", 53),
                    ("Cyan", 74),
                    ("Green", 95),
                    ("Orange", 116),
                ],
            ),
            color_rotate: Bool::new_off("ColorRotate", ()),
            // Ch 2 (upper half): CW 195-255 slow→fast, stop 189-194, CCW 128-188 slow→fast.
            color_rotation: Bipolar::split_channel("ColorRotation", 1, 195, 255, 188, 128, 191)
                .with_detent()
                .with_channel_knob(0),
            // Ch 3: wave motor. CW 134(fast)-255(slow), stop 123-133, CCW 1(slow)-122(fast).
            // Mapped so more knob = faster: cw_slow=255, cw_fast=134; ccw_slow=1, ccw_fast=122.
            wave: Bipolar::split_channel("Wave", 2, 255, 134, 1, 122, 128)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
        }
    }
}

impl AnimatedFixture for Abyss2 {
    type Target = AnimationTarget;

    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>,
    {
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        self.wave.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Wave),
            dmx_buf,
        );
        if self.color_rotate.val() {
            self.color_rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::ColorRotation),
                dmx_buf,
            );
        } else {
            self.fixed_color
                .render(group_controls, std::iter::empty(), dmx_buf);
        }
    }
}
