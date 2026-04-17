//! Control profile for the 100% Chinesium Wizlet, the Chizlet.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 9]
#[strobe(Short)]
pub struct Chizlet {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    drum_swivel: ChannelKnobBipolar<BipolarChannelMirror>,
    #[channel_control]
    #[animate]
    drum_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    gobo: LabeledSelect,
    #[channel_control]
    #[animate]
    reflector_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
}

impl Default for Chizlet {
    fn default() -> Self {
        Self {
            drum_swivel: Bipolar::channel("DrumSwivel", 0, 255, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            drum_rotation: Bipolar::split_channel("DrumRotation", 1, 120, 10, 135, 245, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            gobo: LabeledSelect::new(
                "Gobo",
                2,
                vec![
                    ("Open", 0),
                    ("OrangeCircle", 10),
                    ("WhiteDaisy", 20),
                    ("YellowDots", 30),
                    ("WhiteRing", 40),
                    ("BlueSnow", 50),
                    ("WhiteStar", 60),
                    ("GreenGrid", 70),
                    ("WhiteTris", 80),
                    ("MagentaDotLine", 90),
                ],
            ),
            // FIME: flip fast/slow rotation
            reflector_rotation: Bipolar::split_channel(
                "ReflectorRotation",
                3,
                10,
                120,
                245,
                135,
                0,
            )
            .with_detent()
            .with_mirroring(true)
            .with_channel_knob(2),
            dimmer: Unipolar::full_channel("Dimmer", 5)
                .strobed()
                .with_channel_level(),
        }
    }
}

impl AnimatedFixture for Chizlet {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.drum_swivel.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumSwivel),
            dmx_buf,
        );
        self.drum_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::DrumRotation),
            dmx_buf,
        );
        self.gobo
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.reflector_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::ReflectorRotation),
            dmx_buf,
        );
        dmx_buf[4] = 32; // shutter control - leave open, use dimmer channel
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        dmx_buf[6] = 0; // show
        dmx_buf[7] = 0; // show speed
        dmx_buf[8] = 0; // special; note this can trigger remote fixture reset, might be useful to implement this if they get out of whack
    }
}
