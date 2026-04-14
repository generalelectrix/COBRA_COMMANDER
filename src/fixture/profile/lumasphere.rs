use std::time::Duration;

use crate::fixture::prelude::*;

/// DMX 255 is too fast; restrict to a reasonable value.
const MAX_ROTATION_SPEED: u8 = 100;

/// Control abstraction for the lumapshere.
///
/// lumasphere DMX profile:
///
/// 1: outer ball rotation speed
/// note: requires a value of ~17% in order to be activated
/// (ball start button)
///
/// 2: outer ball rotation direction
/// split halfway
///
/// 3: color wheel rotation
/// (might want to implement bump start)
///
/// 4: strobe 1 intensity
/// 5: strobe 1 rate
/// 6: strobe 2 intensity
/// 7: strobe 2 rate
///
/// There are also two lamp dimmer channels, which are conventionally set to be
/// the two channels after the lumasphere's built-in controller:
/// 8: lamp 1 dimmer
/// 9: lamp 2 dimmer
#[derive(Debug, PatchFixture, Control, EmitState, DescribeControls)]
#[channel_count = 9]
pub struct Lumasphere {
    #[animate]
    lamp_1_intensity: UnipolarChannel,
    #[animate]
    lamp_2_intensity: UnipolarChannel,
    #[channel_control]
    ball_rotation: ChannelKnobBipolar<Bipolar<()>>,
    #[skip_control]
    #[skip_emit]
    ball_current_speed: RampingParameter<BipolarFloat>,
    ball_start: Bool<()>,
    #[channel_control]
    color_rotation: ChannelKnobUnipolar<Unipolar<()>>,
    color_start: Bool<()>,
    strobe_1: Strobe,
    strobe_2: Strobe,
}

impl Default for Lumasphere {
    fn default() -> Self {
        Self {
            lamp_1_intensity: Unipolar::full_channel("Lamp1Intensity", 7),
            lamp_2_intensity: Unipolar::full_channel("Lamp2Intensity", 8),

            ball_rotation: Bipolar::new("BallRotation", ())
                .with_detent()
                .with_channel_knob(0),
            // Ramp ball rotation no faster than unit range in one second.
            ball_current_speed: RampingParameter::new(BipolarFloat::ZERO, BipolarFloat::ONE),
            ball_start: Bool::new_off("BallStart", ()),

            color_rotation: Unipolar::new("ColorRotation", ()).with_channel_knob(1),
            color_start: Bool::new_off("ColorStart", ()),

            strobe_1: Strobe::new("Strobe1"),
            strobe_2: Strobe::new("Strobe2"),
        }
    }
}

impl Lumasphere {
    fn render_ball_rotation(&self, dmx_buf: &mut [u8]) {
        let val = self.ball_current_speed.current().val();
        let mut speed = val.abs();
        let direction = val >= 0.;
        if self.ball_start.val() && speed < 0.2 {
            speed = 0.2;
        }
        let dmx_speed = unipolar_to_range(0, MAX_ROTATION_SPEED, UnipolarFloat::new(speed));
        let dmx_direction = if direction { 0 } else { 255 };
        dmx_buf[0] = dmx_speed;
        dmx_buf[1] = dmx_direction;
    }
}

impl AnimatedFixture for Lumasphere {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.lamp_1_intensity.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Lamp1Intensity),
            dmx_buf,
        );
        self.lamp_2_intensity.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Lamp2Intensity),
            dmx_buf,
        );
        self.render_ball_rotation(dmx_buf);
        // Render color rotation with bump.
        dmx_buf[2] = unipolar_to_range(
            0,
            255,
            if self.color_start.val() && self.color_rotation.control.val() < 0.2 {
                UnipolarFloat::new(0.2)
            } else {
                self.color_rotation.control.val()
            },
        );
        self.strobe_1.render(&mut dmx_buf[3..5]);
        self.strobe_2.render(&mut dmx_buf[5..7]);
    }
}

impl Update for Lumasphere {
    fn update(&mut self, _: FixtureGroupUpdate, delta_t: Duration) {
        self.ball_current_speed.target = self.ball_rotation.control.val();
        self.ball_current_speed.update(delta_t);
    }
}

#[derive(Debug, Control, EmitState, DescribeControls)]
struct Strobe {
    on: Bool<()>,
    rate: Unipolar<()>,
    intensity: Unipolar<()>,
}

impl Strobe {
    fn new(prefix: &str) -> Self {
        Self {
            on: Bool::new_off(format!("{prefix}On"), ()),
            rate: Unipolar::new(format!("{prefix}Rate"), ()),
            intensity: Unipolar::new(format!("{prefix}Intensity"), ()),
        }
    }

    fn render(&self, dmx_slice: &mut [u8]) {
        let (intensity, rate) = if self.on.val() {
            (
                unipolar_to_range(0, 255, self.intensity.val()),
                unipolar_to_range(0, 255, self.rate.val()),
            )
        } else {
            (0, 0)
        };
        dmx_slice[0] = intensity;
        dmx_slice[1] = rate;
    }
}
