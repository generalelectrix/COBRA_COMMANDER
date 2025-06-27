use anyhow::anyhow;
use tunnels::clock_bank::{ClockIdxExt, N_CLOCKS};

use crate::animation::AnimationUIState;
use crate::animation::ControlMessage as AnimationControlMessage;

use crate::fixture::animation_target::N_ANIM;
use crate::osc::RadioButton;

use tunnels::animation::{ControlMessage, StateChange, Waveform::*};

use super::basic_controls::{button, Button};
use super::label_array::LabelArray;
use super::GroupControlMap;

pub(crate) const GROUP: &str = "Animation";

// Base animation system

// knobs
const SPEED: &str = "Speed";
const SIZE: &str = "Size";
const DUTY_CYCLE: &str = "DutyCycle";
const SMOOTHING: &str = "Smoothing";

// assorted parameters
const PULSE: Button = button("Pulse");
const INVERT: Button = button("Invert");
const USE_AUDIO_SIZE: Button = button("UseAudioSize");
const USE_AUDIO_SPEED: Button = button("UseAudioSpeed");
const STANDING: Button = button("Standing");

// clipboard
const COPY: Button = button("Copy");
const PASTE: Button = button("Paste");

const WAVEFORM_SELECT: RadioButton = RadioButton {
    control: "Waveform",
    n: 6,
    x_primary_coordinate: false,
};

const N_PERIODS_SELECT: RadioButton = RadioButton {
    control: "NPeriods",
    n: 16,
    x_primary_coordinate: false,
};

const CLOCK_SOURCE: RadioButton = RadioButton {
    control: "ClockSource",
    n: N_CLOCKS + 1,
    x_primary_coordinate: false,
};

impl AnimationUIState {
    pub fn map_controls(map: &mut GroupControlMap<AnimationControlMessage>) {
        use crate::animation::ControlMessage::Animation as WrapAnimation;
        use ControlMessage::*;
        use StateChange::*;
        WAVEFORM_SELECT.map_fallible(map, |v| {
            match v {
                0 => Some(Sine),
                1 => Some(Triangle),
                2 => Some(Square),
                3 => Some(Sawtooth),
                4 => Some(Noise),
                5 => Some(Constant),
                _ => None,
            }
            .map(|waveform| WrapAnimation(Set(Waveform(waveform))))
            .ok_or_else(|| anyhow!("waveform select out of range: {v}"))
        });

        map.add_bipolar(SPEED, |v| WrapAnimation(Set(Speed(v))));
        map.add_unipolar(SIZE, |v| WrapAnimation(Set(Size(v))));
        map.add_unipolar(DUTY_CYCLE, |v| WrapAnimation(Set(DutyCycle(v))));
        map.add_unipolar(SMOOTHING, |v| WrapAnimation(Set(Smoothing(v))));

        N_PERIODS_SELECT.map(map, |v| WrapAnimation(Set(NPeriods(v as u16))));
        CLOCK_SOURCE.map(map, |v| {
            if v == 0 {
                WrapAnimation(SetClockSource(None))
            } else {
                WrapAnimation(SetClockSource(Some(ClockIdxExt(v - 1))))
            }
        });
        PULSE.map_trigger(map, || WrapAnimation(TogglePulse));
        INVERT.map_trigger(map, || WrapAnimation(ToggleInvert));
        STANDING.map_trigger(map, || WrapAnimation(ToggleStanding));
        USE_AUDIO_SPEED.map_trigger(map, || WrapAnimation(ToggleUseAudioSpeed));
        USE_AUDIO_SIZE.map_trigger(map, || WrapAnimation(ToggleUseAudioSize));

        ANIMATION_TARGET_SELECT.map(map, AnimationControlMessage::Target);
        ANIMATION_SELECT.map(map, AnimationControlMessage::SelectAnimation);

        COPY.map_trigger(map, || AnimationControlMessage::Copy);
        PASTE.map_trigger(map, || AnimationControlMessage::Paste);
    }
}

// Targeting/selection

const N_ANIM_TARGET: usize = 8;

const ANIMATION_SELECT: RadioButton = RadioButton {
    control: "Select",
    n: N_ANIM,
    x_primary_coordinate: false,
};

const ANIMATION_TARGET_SELECT: RadioButton = RadioButton {
    control: "Target",
    n: N_ANIM_TARGET,
    x_primary_coordinate: false,
};

const ANIMATION_TARGET_LABELS: LabelArray = LabelArray {
    control: "TargetLabel",
    n: N_ANIM_TARGET,
    empty_label: "",
};

impl AnimationUIState {
    pub fn emit_osc_state_change<S>(sc: crate::animation::StateChange, send: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        match sc {
            crate::animation::StateChange::Animation(msg) => {
                Self::emit_nested_osc_state_change(msg, send)
            }
            crate::animation::StateChange::SelectAnimation(msg) => ANIMATION_SELECT.set(msg, send),
            crate::animation::StateChange::Target(msg) => ANIMATION_TARGET_SELECT.set(msg, send),
            crate::animation::StateChange::TargetLabels(labels) => {
                ANIMATION_TARGET_LABELS.set(labels.into_iter(), send)
            }
        }
    }

    fn emit_nested_osc_state_change<S>(sc: StateChange, emitter: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        use StateChange::*;
        match sc {
            Waveform(v) => WAVEFORM_SELECT.set(
                match v {
                    Sine => 0,
                    Triangle => 1,
                    Square => 2,
                    Sawtooth => 3,
                    Constant => 4,
                    Noise => 5,
                },
                emitter,
            ),
            Speed(v) => emitter.emit_float(SPEED, v.into()),
            Size(v) => emitter.emit_float(SIZE, v.into()),
            DutyCycle(v) => emitter.emit_float(DUTY_CYCLE, v.into()),
            Smoothing(v) => emitter.emit_float(SMOOTHING, v.into()),

            NPeriods(v) => N_PERIODS_SELECT.set(v as usize, emitter),
            ClockSource(maybe_clock) => CLOCK_SOURCE.set(
                maybe_clock.map(|v| usize::from(v) + 1).unwrap_or(0),
                emitter,
            ),

            Pulse(v) => PULSE.send(v, emitter),
            Standing(v) => STANDING.send(v, emitter),
            Invert(v) => INVERT.send(v, emitter),
            UseAudioSize(v) => USE_AUDIO_SIZE.send(v, emitter),
            UseAudioSpeed(v) => USE_AUDIO_SPEED.send(v, emitter),
        }
    }
}
