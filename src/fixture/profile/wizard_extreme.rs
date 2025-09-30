//! Martin Wizard Extreme - the one that Goes Slow
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 11]
pub struct WizardExtreme {
    #[channel_control]
    #[animate]
    shutter: ChannelLevelUnipolar<DimmerStrobe>,
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

impl Default for WizardExtreme {
    fn default() -> Self {
        Self {
            shutter: ShutterStrobe::new(
                Unipolar::channel("Dimmer", 0, 0, 129),
                Strobe::channel("Strobe", 0, 189, 130, 0),
            )
            .with_channel_level(),
            color: LabeledSelect::new(
                "Color",
                2,
                vec![
                    ("Open", 0),
                    ("Blue", 12),
                    ("Orange", 24),
                    ("Purple", 36),
                    ("Green", 48),
                    ("DarkBlue", 60),
                    ("Yellow", 72),
                    ("Magenta", 84),
                ],
            )
            .with_split(6),
            twinkle: Bool::new_off("Twinkle", ()),
            twinkle_speed: Unipolar::channel("TwinkleSpeed", 2, 176, 243),
            // 14 gobos, including the open position
            gobo: IndexedSelect::multiple("Gobo", 4, false, 14, 12, 0),
            drum_swivel: Bipolar::channel("DrumSwivel", 6, 0, 127)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            drum_rotation: Bipolar::split_channel("DrumRotation", 7, 2, 63, 127, 66, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            reflector_rotation: Bipolar::split_channel("ReflectorRotation", 1, 2, 63, 127, 66, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),
        }
    }
}

impl ControllableFixture for WizardExtreme {}

impl AnimatedFixture for WizardExtreme {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.shutter.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Shutter),
            dmx_buf,
        );
        self.reflector_rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::ReflectorRotation),
            dmx_buf,
        );
        if self.twinkle.val() {
            self.twinkle_speed.render(
                animation_vals.filter(&AnimationTarget::TwinkleSpeed),
                dmx_buf,
            );
        } else {
            self.color.render_no_anim(dmx_buf);
        }
        self.gobo.render_no_anim(dmx_buf);

        self.drum_swivel.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumSwivel),
            dmx_buf,
        );
        self.drum_rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumRotation),
            dmx_buf,
        );
        dmx_buf[3] = 0; // color shake
        dmx_buf[5] = 0; // gobo shake
        dmx_buf[8] = 0;
        dmx_buf[9] = 0;
        dmx_buf[10] = 0;
    }
}
