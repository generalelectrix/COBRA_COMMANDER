//! OSC control mappings for the tunnels clock system.
use anyhow::Result;
use tunnels::clock::ControlMessage::*;
use tunnels::clock::StateChange::*;
use tunnels::clock_bank::{ClockIdxExt, ControlMessage, StateChange};

use crate::osc::prelude::*;

pub const GROUP: &str = "Clock";

// knobs
const RATE: BipolarArray = bipolar_array("Rate");
const RATE_FINE: BipolarArray = bipolar_array("RateFine");
const LEVEL: UnipolarArray = unipolar_array("Level");

// buttons
const TAP: ButtonArray = button_array("Tap");
const ONE_SHOT: ButtonArray = button_array("OneShot");
const RETRIGGER: ButtonArray = button_array("Retrigger");
const USE_AUDIO_SIZE: ButtonArray = button_array("UseAudioSize");
const USE_AUDIO_SPEED: ButtonArray = button_array("UseAudioSpeed");

pub fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    RATE.map(map, channel_control(|v| Set(Rate(v))));
    RATE_FINE.map(map, channel_control(|v| Set(RateFine(v))));
    LEVEL.map(map, channel_control(|v| Set(SubmasterLevel(v))));

    TAP.map(map, channel_button(|| Tap));
    ONE_SHOT.map(map, channel_button(|| ToggleOneShot));
    RETRIGGER.map(map, channel_button(|| Retrigger));
    USE_AUDIO_SIZE.map(map, channel_button(|| ToggleUseAudioSize));
    USE_AUDIO_SPEED.map(map, channel_button(|| ToggleUseAudioSpeed));
}

pub fn emit_osc_state_change<S>(sc: &StateChange, emitter: &S)
where
    S: crate::osc::EmitScopedOscMessage + ?Sized,
{
    let channel: usize = sc.channel.into();
    match sc.change {
        Rate(v) => RATE.set(channel, v, emitter),
        RateFine(v) => RATE_FINE.set(channel, v, emitter),
        SubmasterLevel(v) => LEVEL.set(channel, v, emitter),
        OneShot(v) => ONE_SHOT.set(channel, v, emitter),
        UseAudioSize(v) => USE_AUDIO_SIZE.set(channel, v, emitter),
        UseAudioSpeed(v) => USE_AUDIO_SPEED.set(channel, v, emitter),
        Ticked(v) => TAP.set(channel, v, emitter),
    }
}

fn channel_control<T>(
    proc: impl Fn(T) -> tunnels::clock::ControlMessage + 'static + Copy,
) -> impl Fn(usize, T) -> Result<ControlMessage> + Copy {
    move |i: usize, v: T| {
        let control = proc(v);
        Ok(ControlMessage {
            channel: ClockIdxExt(i),
            msg: control,
        })
    }
}

fn channel_button(
    proc: impl Fn() -> tunnels::clock::ControlMessage + 'static + Copy,
) -> impl Fn(usize) -> ControlMessage + Copy {
    move |i: usize| {
        let control = proc();
        ControlMessage {
            channel: ClockIdxExt(i),
            msg: control,
        }
    }
}
