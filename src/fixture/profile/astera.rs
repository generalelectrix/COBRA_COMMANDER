//! Control profile for Astera LEDs running in RC Wireless mode.
use super::color::Model::Rgb;

use crate::fixture::prelude::*;

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
    hue1: PhaseControl<()>,
    sat1: Unipolar<()>,
    val1: Unipolar<()>,
    hue2: PhaseControl<()>,
    sat2: Unipolar<()>,
    val2: Unipolar<()>,
    hue3: PhaseControl<()>,
    sat3: Unipolar<()>,
    val3: Unipolar<()>,
    hue4: PhaseControl<()>,
    sat4: Unipolar<()>,
    val4: Unipolar<()>,
}

impl Default for Astera {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 0).with_channel_level(),
            speed: Unipolar::full_channel("Speed", 3).with_channel_knob(0),
            fade: Unipolar::full_channel("Fade", 5).with_channel_knob(1),
            program: LabeledSelect::new(
                "Program",
                3,
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
            hue1: PhaseControl::new("Hue1", ()),
            sat1: Unipolar::new("Sat1", ()),
            val1: Unipolar::new("Val1", ()),
            hue2: PhaseControl::new("Hue2", ()),
            sat2: Unipolar::new("Sat2", ()),
            val2: Unipolar::new("Val2", ()),
            hue3: PhaseControl::new("Hue3", ()),
            sat3: Unipolar::new("Sat3", ()),
            val3: Unipolar::new("Val3", ()),
            hue4: PhaseControl::new("Hue4", ()),
            sat4: Unipolar::new("Sat4", ()),
            val4: Unipolar::new("Val4", ()),
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
        self.speed
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.fade
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
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
        Rgb.render(
            &mut dmx_buf[8..11],
            self.hue1.val(),
            self.sat1.val(),
            self.val1.val(),
        );
        Rgb.render(
            &mut dmx_buf[11..14],
            self.hue2.val(),
            self.sat2.val(),
            self.val2.val(),
        );
        Rgb.render(
            &mut dmx_buf[14..17],
            self.hue3.val(),
            self.sat3.val(),
            self.val3.val(),
        );
        Rgb.render(
            &mut dmx_buf[17..20],
            self.hue4.val(),
            self.sat4.val(),
            self.val4.val(),
        );
    }
}

impl ControllableFixture for Astera {}
