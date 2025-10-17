//! RIP the Venus, perhaps someday it will fly again.

use std::time::Duration;

use crate::fixture::prelude::*;

/// Control abstraction for the RA venus.
/// DMX profile Venus
///
/// Motor 1 is base motor
/// Motor 2 is crescent translate motor
/// Motor 3 is saucer off axis rotate motor
/// Motor 4 is color carousel
///
/// Motor direction is split at 127
/// Lamp on/off is split at 127 (high is on)
///
/// 1 - Motor 1 Dir
/// 2 - Motor 1 Speed
/// 3 - Motor 2 Speed
/// 4 - Motor 3 Dir
/// 5 - Motor 3 Speed
/// 6 - Motor 4 Dir
/// 7 - Motor 4 Speed
/// 8 - Lamp Control
#[derive(Debug, PatchFixture)]
#[channel_count = 8]
pub struct Venus {
    controls: GroupControlMap<ControlMessage>,
    base_rotation: RampingParameter<BipolarFloat>,
    cradle_motion: RampingParameter<UnipolarFloat>,
    head_rotation: RampingParameter<BipolarFloat>,
    color_rotation: RampingParameter<BipolarFloat>,
    lamp_on: bool,
}

impl Default for Venus {
    fn default() -> Self {
        Self {
            controls: map_controls(),
            base_rotation: RampingParameter::new(BipolarFloat::ZERO, BipolarFloat::ONE),
            cradle_motion: RampingParameter::new(UnipolarFloat::ZERO, UnipolarFloat::ONE),
            head_rotation: RampingParameter::new(BipolarFloat::ZERO, BipolarFloat::ONE),
            color_rotation: RampingParameter::new(BipolarFloat::ZERO, BipolarFloat::ONE),
            lamp_on: false,
        }
    }
}

impl Venus {
    fn handle_state_change(&mut self, sc: StateChange, emitter: &FixtureStateEmitter) {
        use StateChange::*;
        match sc {
            BaseRotation(v) => self.base_rotation.target = v,
            CradleMotion(v) => self.cradle_motion.target = v,
            HeadRotation(v) => self.head_rotation.target = v,
            ColorRotation(v) => self.color_rotation.target = v,
            LampOn(v) => self.lamp_on = v,
        };
        Self::emit(sc, emitter);
    }

    fn emit(_sc: StateChange, _emitter: &FixtureStateEmitter) {
        // FIXME: no talkback
    }
}

impl NonAnimatedFixture for Venus {
    fn render(&self, _group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        render_bipolar_to_dir_and_val(self.base_rotation.current(), &mut dmx_buf[0..2]);
        dmx_buf[2] = unipolar_to_range(0, 255, self.cradle_motion.current());
        render_bipolar_to_dir_and_val(self.head_rotation.current(), &mut dmx_buf[3..5]);
        // Limit color wheel speed to 50% (...it still chewed itself to pieces...).
        let color_wheel_scale = UnipolarFloat::new(0.5);
        render_bipolar_to_dir_and_val(
            self.color_rotation.current() * color_wheel_scale,
            &mut dmx_buf[5..7],
        );
        dmx_buf[7] = if self.lamp_on { 255 } else { 0 };
    }
}

impl crate::fixture::EmitState for Venus {
    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        use StateChange::*;
        Self::emit(BaseRotation(self.base_rotation.target), emitter);
        Self::emit(CradleMotion(self.cradle_motion.target), emitter);
        Self::emit(HeadRotation(self.head_rotation.target), emitter);
        Self::emit(ColorRotation(self.color_rotation.target), emitter);
        Self::emit(LampOn(self.lamp_on), emitter);
    }
}

impl crate::fixture::Control for Venus {
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

    fn control_from_channel(
        &mut self,
        _msg: &crate::channel::ChannelControlMessage,
        _emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn can_strobe(&self) -> bool {
        false
    }
}

impl Update for Venus {
    fn update(&mut self, _: &MasterControls, delta_t: Duration) {
        self.base_rotation.update(delta_t);
        self.cradle_motion.update(delta_t);
        self.head_rotation.update(delta_t);
        self.color_rotation.update(delta_t);
    }
}

fn render_bipolar_to_dir_and_val(v: BipolarFloat, out: &mut [u8]) {
    out[1] = unipolar_to_range(0, 255, v.abs());
    out[0] = if v.val() < 0.0 { 0 } else { 255 };
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    BaseRotation(BipolarFloat),
    CradleMotion(UnipolarFloat),
    HeadRotation(BipolarFloat),
    ColorRotation(BipolarFloat),
    LampOn(bool),
}

pub type ControlMessage = StateChange;

const LAMP_ON: Button = button("LampControl");

fn map_controls() -> GroupControlMap<ControlMessage> {
    let mut controls = GroupControlMap::default();
    let map = &mut controls;
    use StateChange::*;

    map.add_bipolar("BaseRotation", |v| {
        BaseRotation(bipolar_fader_with_detent(v))
    });
    map.add_unipolar("CradleMotion", |v| {
        CradleMotion(unipolar_fader_with_detent(v))
    });
    map.add_bipolar("HeadRotation", |v| {
        HeadRotation(bipolar_fader_with_detent(v))
    });
    map.add_bipolar("ColorRotation", |v| {
        ColorRotation(bipolar_fader_with_detent(v))
    });
    LAMP_ON.map_state(map, LampOn);
    controls
}
