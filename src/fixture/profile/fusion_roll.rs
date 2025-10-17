use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 11]
pub struct FusionRoll {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    drum_swivel: ChannelKnobBipolar<BipolarChannelMirror>,
    #[channel_control]
    #[animate]
    drum_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    color: LabeledSelect,

    #[channel_control]
    #[animate]
    laser_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    led_strobe: StrobeChannel,
    laser: FullShutterStrobe,
}

impl Default for FusionRoll {
    fn default() -> Self {
        Self {
            drum_swivel: Bipolar::channel("DrumSwivel", 0, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            drum_rotation: Bipolar::split_channel("DrumRotation", 1, 10, 120, 245, 135, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),

            color: LabeledSelect::new(
                "Color",
                2,
                vec![
                    ("Open", 0),
                    ("Red", 8),
                    ("Orange", 16),
                    ("Yellow", 24),
                    ("Green", 32),
                    ("Blue", 40),
                    ("LightBlue", 48),
                    ("Pink", 56),
                ],
            )
            .with_split(56),
            dimmer: Unipolar::full_channel("Dimmer", 4).with_channel_level(),

            laser_rotation: Bipolar::split_channel("LaserRotation", 5, 10, 120, 136, 245, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),
            led_strobe: Strobe::channel("LEDStrobe", 3, 16, 131, 8),
            laser: FullShutterStrobe::new(
                Bool::channel("LaserOn", 6, 0, 8),
                Strobe::channel("LaserStrobe", 6, 16, 131, 8),
            ),
        }
    }
}

impl AnimatedFixture for FusionRoll {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
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
        self.color.render_no_anim(dmx_buf);
        self.laser_rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::LaserRotation),
            dmx_buf,
        );
        self.led_strobe
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.dimmer.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        self.laser
            .render(group_controls, std::iter::empty(), dmx_buf);
    }
}
