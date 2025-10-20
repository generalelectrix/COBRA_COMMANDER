//! Martin Rush-series Wizard (still not as good as the OG).

use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 10]
#[strobe]
pub struct RushWizard {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    // TODO: figure out what's up with this fixture/how we should strobe it
    strobe: StrobeChannel,
    color: LabeledSelect,
    twinkle: Bool<()>,
    #[animate]
    twinkle_speed: UnipolarChannel,
    gobo: IndexedSelectMult,
    #[channel_control]
    #[animate]
    drum_swivel: ChannelKnobBipolar<BipolarChannelMirror>,
    #[channel_control]
    #[animate]
    drum_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[channel_control]
    #[animate]
    reflector_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for RushWizard {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 1).with_channel_level(),
            strobe: Strobe::channel("Strobe", 0, 16, 131, 8),
            color: LabeledSelect::new(
                "Color",
                2,
                vec![
                    ("Open", 159),
                    ("Blue", 161),
                    ("Magenta", 164),
                    ("Yellow", 167),
                    ("DarkBlue", 170),
                    ("White", 173),
                    ("Red", 176),
                    ("Orange", 179),
                    ("Green", 182),
                ],
            ),
            twinkle: Bool::new_off("Twinkle", ()),
            twinkle_speed: Unipolar::channel("TwinkleSpeed", 2, 221, 243),
            // 16 gobos, including the open position
            gobo: IndexedSelect::multiple("Gobo", 3, false, 16, 2, 160),
            drum_swivel: Bipolar::channel("DrumSwivel", 5, 0, 120)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            drum_rotation: Bipolar::split_channel("DrumRotation", 4, 190, 128, 193, 255, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),

            reflector_rotation: Bipolar::split_channel(
                "ReflectorRotation",
                6,
                190,
                128,
                193,
                255,
                191,
            )
            .with_detent()
            .with_mirroring(true)
            .with_channel_knob(2),
        }
    }
}

impl AnimatedFixture for RushWizard {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.strobe
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        if self.twinkle.val() {
            self.twinkle_speed.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::TwinkleSpeed),
                dmx_buf,
            );
        } else {
            self.color
                .render(group_controls, std::iter::empty(), dmx_buf);
        }
        self.gobo
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.drum_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumRotation),
            dmx_buf,
        );
        self.drum_swivel.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumSwivel),
            dmx_buf,
        );
        self.reflector_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::ReflectorRotation),
            dmx_buf,
        );
        dmx_buf[7] = 0;
        dmx_buf[8] = 0;
        dmx_buf[9] = 0;
    }
}
