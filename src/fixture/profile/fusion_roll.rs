use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 11]
#[strobe(Short)]
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
    led_strobe_on: Bool<()>,
    laser_on: BoolChannel,
    laser_strobe_on: Bool<()>,
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
            led_strobe_on: Bool::new_off("LEDStrobeOn", ()),

            laser_rotation: Bipolar::split_channel("LaserRotation", 5, 10, 120, 136, 245, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(2),

            laser_on: Bool::channel("LaserOn", 6, 0, 8),
            laser_strobe_on: Bool::new_off("LaserStrobeOn", ()),
        }
    }
}

impl FusionRoll {
    fn render_led_intensity(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<AnimationTarget>,
        dmx_buf: &mut [u8],
    ) {
        if self.led_strobe_on.val()
            && group_controls.strobe_enabled
            && let Some(strobe_override) = group_controls.strobe_intensity()
        {
            dmx_buf[4] = unipolar_to_range(0, 255, strobe_override);
            return;
        }
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
    }

    fn render_laser_state(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        if self.laser_strobe_on.val()
            && group_controls.strobe_enabled
            && let Some(flash_on) = group_controls.strobe_shutter()
        {
            dmx_buf[6] = if flash_on { 8 } else { 0 };
            return;
        }
        self.laser_on
            .render(group_controls, std::iter::empty(), dmx_buf);
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
        self.color
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.render_led_intensity(group_controls, animation_vals, dmx_buf);
        self.laser_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::LaserRotation),
            dmx_buf,
        );
        self.render_laser_state(group_controls, dmx_buf);
    }
}
