//! OSC control mappings for the tunnels clock system.

use tunnels::clock_bank::{ControlMessage, StateChange};

use crate::osc::prelude::*;

pub const GROUP: &str = "Clock";

// knobs
const RATE: &str = "Rate";
const RATE_FINE: &str = "RateFine";
const LEVEL: UnipolarArray = unipolar_array("Level");

// buttons
const TAP: ButtonArray = button_array("Tap");
const ONE_SHOT: ButtonArray = button_array("OneShot");
const RETRIGGER: ButtonArray = button_array("Retrigger");
const USE_AUDIO_SIZE: ButtonArray = button_array("UseAudioSize");
const USE_AUDIO_SPEED: ButtonArray = button_array("UseAudioSpeed");

pub fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    todo!()
}

pub fn emit_osc_state_change<S>(sc: &StateChange, emitter: &S)
where
    S: crate::osc::EmitScopedOscMessage + ?Sized,
{
    todo!()
}
