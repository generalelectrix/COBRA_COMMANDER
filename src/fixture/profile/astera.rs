//! Control profile for Astera LEDs running in RC Wireless mode.
use crate::{
    color::ColorSpace,
    fixture::{color::Color, prelude::*},
};

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 20]
pub struct Astera {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    speed: ChannelKnobUnipolar<UnipolarChannel>,
    #[channel_control]
    fade: ChannelKnobUnipolar<UnipolarChannel>,
    program: LabeledSelect,
    pattern_direction: Bool<()>,
    pattern_loop: Bool<()>,
    #[force_osc_control]
    color1: Color,
    #[force_osc_control]
    color2: Color,
    #[force_osc_control]
    color3: Color,
    #[force_osc_control]
    color4: Color,
}

impl Default for Astera {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 0).with_channel_level(),
            speed: Unipolar::full_channel("Speed", 3).with_channel_knob(0),
            fade: Unipolar::full_channel("Fade", 4).with_channel_knob(1),
            program: LabeledSelect::new(
                "Program",
                2,
                vec![
                    ("Static1", 0),
                    // ("Static2", 7),
                    // ("Static3", 15),
                    // ("Static4", 23),
                    ("Fade1", 31),
                    ("Fade2", 39),
                    ("Fade3", 47),
                    ("Fade4", 55),
                    // ("Run1", 63),
                    // ("Run2", 71),
                    // ("Run2Col", 79),
                    // ("RunFlag", 87),
                    // ("RunFlag2", 95),
                    ("Spiral2", 111),
                    ("Spiral4", 103),
                    ("Fire", 127),
                    // ("Rotor", 135),
                    // ("Rotor2", 143),
                    // ("Rotor4", 151),
                ],
            ),
            pattern_direction: Bool::new_on("Forward", ()),
            pattern_loop: Bool::new_on("Loop", ()),
            color1: Color::for_subcontrol(Some(1), ColorSpace::Hsv),
            color2: Color::for_subcontrol(Some(2), ColorSpace::Hsv),
            color3: Color::for_subcontrol(Some(3), ColorSpace::Hsv),
            color4: Color::for_subcontrol(Some(4), ColorSpace::Hsv),
        }
    }
}

impl AnimatedFixture for Astera {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.dimmer.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        dmx_buf[1] = 0; // strobe off
        self.speed.render_no_anim(dmx_buf);
        self.fade.render_no_anim(dmx_buf);
        self.program.render_no_anim(dmx_buf);
        dmx_buf[5] = match (self.pattern_direction.val(), self.pattern_loop.val()) {
            (true, true) => 0,
            (true, false) => 64,
            (false, false) => 128,
            (false, true) => 191,
        };
        // TODO: send to groups
        dmx_buf[6] = 0;
        dmx_buf[7] = 0; // send on modify

        self.color1
            .render_without_animations(super::color::Model::Rgb, &mut dmx_buf[8..11]);
        self.color2
            .render_without_animations(super::color::Model::Rgb, &mut dmx_buf[11..14]);
        self.color3
            .render_without_animations(super::color::Model::Rgb, &mut dmx_buf[14..17]);
        self.color4
            .render_without_animations(super::color::Model::Rgb, &mut dmx_buf[17..20]);
    }
}

impl ControllableFixture for Astera {}
