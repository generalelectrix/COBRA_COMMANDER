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
#[derive(Debug, PatchFixture)]
#[channel_count = 9]
pub struct Lumasphere {
    controls: GroupControlMap<ControlMessage>,
    lamp_1_intensity: UnipolarFloat,
    lamp_2_intensity: UnipolarFloat,
    ball_rotation: RampingParameter<BipolarFloat>,
    ball_start: bool,
    color_rotation: UnipolarFloat,
    color_start: bool,
    strobe_1: Strobe,
    strobe_2: Strobe,
}

impl Default for Lumasphere {
    fn default() -> Self {
        Self {
            controls: map_controls(),
            lamp_1_intensity: UnipolarFloat::ZERO,
            lamp_2_intensity: UnipolarFloat::ZERO,
            // Ramp ball rotation no faster than unit range in one second.
            ball_rotation: RampingParameter::new(BipolarFloat::ZERO, BipolarFloat::ONE),
            ball_start: false,
            color_rotation: UnipolarFloat::ZERO,
            color_start: false,
            strobe_1: Strobe::default(),
            strobe_2: Strobe::default(),
        }
    }
}

impl Lumasphere {
    fn render_ball_rotation(&self, dmx_slice: &mut [u8]) {
        let val = self.ball_rotation.current().val();
        let mut speed = val.abs();
        let direction = val >= 0.;
        if self.ball_start && speed < 0.2 {
            speed = 0.2;
        }
        let dmx_speed = unipolar_to_range(0, MAX_ROTATION_SPEED, UnipolarFloat::new(speed));
        let dmx_direction = if direction { 0 } else { 255 };
        dmx_slice[0] = dmx_speed;
        dmx_slice[1] = dmx_direction;
    }

    fn render_color_rotation(&self) -> u8 {
        let speed = if self.color_start && self.color_rotation.val() < 0.2 {
            UnipolarFloat::new(0.2)
        } else {
            self.color_rotation
        };
        unipolar_to_range(0, 255, speed)
    }

    fn handle_state_change(&mut self, sc: StateChange, emitter: &FixtureStateEmitter) {
        use StateChange::*;
        match sc {
            Lamp1Intensity(v) => self.lamp_1_intensity = v,
            Lamp2Intensity(v) => self.lamp_2_intensity = v,
            BallRotation(v) => self.ball_rotation.target = v,
            BallStart(v) => self.ball_start = v,
            ColorRotation(v) => self.color_rotation = v,
            ColorStart(v) => self.color_start = v,
            Strobe1(sc) => self.strobe_1.handle_state_change(&sc),

            Strobe2(sc) => self.strobe_2.handle_state_change(&sc),
        };
        Self::emit(sc, emitter);
    }

    fn emit(_sc: StateChange, _emitter: &FixtureStateEmitter) {
        // FIXME: no talkback
    }
}

impl NonAnimatedFixture for Lumasphere {
    fn render(&self, _group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        self.render_ball_rotation(&mut dmx_buf[0..2]);
        dmx_buf[2] = self.render_color_rotation();
        self.strobe_1.render(&mut dmx_buf[3..5]);
        self.strobe_2.render(&mut dmx_buf[5..7]);
        dmx_buf[7] = unipolar_to_range(0, 255, self.lamp_1_intensity);
        dmx_buf[8] = unipolar_to_range(0, 255, self.lamp_2_intensity);
    }
}

impl crate::fixture::EmitState for Lumasphere {
    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        use StateChange::*;
        Self::emit(Lamp1Intensity(self.lamp_1_intensity), emitter);
        Self::emit(Lamp2Intensity(self.lamp_2_intensity), emitter);
        Self::emit(BallRotation(self.ball_rotation.current()), emitter);
        Self::emit(BallStart(self.ball_start), emitter);
        Self::emit(ColorRotation(self.color_rotation), emitter);
        Self::emit(ColorStart(self.color_start), emitter);
        self.strobe_1.emit_state(emitter, Strobe1);
        self.strobe_2.emit_state(emitter, Strobe2);
    }
}

impl crate::fixture::Control for Lumasphere {
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        let Some((ctl, _)) = self.controls.handle(msg)? else {
            return Ok(true);
        };
        self.handle_state_change(ctl, emitter);
        Ok(true)
    }
}

impl ControllableFixture for Lumasphere {
    fn update(&mut self, _: &MasterControls, delta_t: Duration) {
        self.ball_rotation.update(delta_t);
    }
}

#[derive(Default, Debug)]
pub struct Strobe {
    state: GenericStrobe,
    intensity: UnipolarFloat,
}

impl Strobe {
    fn render(&self, dmx_slice: &mut [u8]) {
        let (intensity, rate) = if self.state.on() {
            (
                unipolar_to_range(0, 255, self.intensity),
                unipolar_to_range(0, 255, self.state.rate()),
            )
        } else {
            (0, 0)
        };
        dmx_slice[0] = intensity;
        dmx_slice[1] = rate;
    }

    fn emit_state<F>(&self, emitter: &FixtureStateEmitter, wrap: F)
    where
        F: Fn(StrobeStateChange) -> StateChange + 'static,
    {
        use StrobeStateChange::*;
        Lumasphere::emit(wrap(Intensity(self.intensity)), emitter);
        let mut emit = |ssc| {
            Lumasphere::emit(wrap(State(ssc)), emitter);
        };
        self.state.emit_state(&mut emit);
    }

    fn handle_state_change(&mut self, sc: &StrobeStateChange) {
        use StrobeStateChange::*;
        match sc {
            State(v) => self.state.handle_state_change(v),
            Intensity(v) => self.intensity = *v,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StrobeStateChange {
    Intensity(UnipolarFloat),
    State(GenericStrobeStateChange),
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    Lamp1Intensity(UnipolarFloat),
    Lamp2Intensity(UnipolarFloat),
    BallRotation(BipolarFloat),
    BallStart(bool),
    ColorRotation(UnipolarFloat),
    ColorStart(bool),
    Strobe1(StrobeStateChange),
    Strobe2(StrobeStateChange),
}

// Lumasphere has no controls that are not represented as state changes.
pub type ControlMessage = StateChange;

const BALL_START: Button = button("ball_start");
const COLOR_START: Button = button("color_start");

fn map_controls() -> GroupControlMap<ControlMessage> {
    let mut controls = GroupControlMap::default();
    let map = &mut controls;
    use StateChange::*;
    map.add_unipolar("lamp_1_intensity", |v| {
        Lamp1Intensity(unipolar_fader_with_detent(v))
    });
    map.add_unipolar("lamp_2_intensity", |v| {
        Lamp2Intensity(unipolar_fader_with_detent(v))
    });

    map.add_bipolar("ball_rotation", |v| {
        BallRotation(bipolar_fader_with_detent(v))
    });
    BALL_START.map_state(map, BallStart);

    map.add_unipolar("color_rotation", |v| {
        ColorRotation(unipolar_fader_with_detent(v))
    });
    COLOR_START.map_state(map, ColorStart);
    map_strobe(map, 1, Strobe1);
    map_strobe(map, 2, Strobe2);
    controls
}

fn map_strobe<W>(map: &mut GroupControlMap<ControlMessage>, index: u8, wrap: W)
where
    W: Fn(StrobeStateChange) -> ControlMessage + 'static + Copy,
{
    use GenericStrobeStateChange::*;
    use StrobeStateChange::*;
    map.add_bool(&format!("strobe_{}_state", index), move |v| {
        wrap(State(On(v)))
    });
    map.add_unipolar(&format!("strobe_{}_rate", index), move |v| {
        wrap(State(Rate(v)))
    });
    map.add_unipolar(&format!("strobe_{}_intensity", index), move |v| {
        wrap(Intensity(v))
    });
}
