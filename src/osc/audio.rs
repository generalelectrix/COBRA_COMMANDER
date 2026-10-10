//! OSC control mappings for the tunnels audio system.

use std::time::Duration;

use number::UnipolarFloat;
use tunnels::audio::{ControlMessage, StateChange};

use super::{
    GroupControlMap,
    prelude::{Button, button},
};

pub const GROUP: &str = "Audio";

const MONITOR_TOGGLE: Button = button("Monitor");
const RESET: Button = button("Reset");

// Knobs
const ENVELOPE_ATTACK: &str = "EnvelopeAttack";
const ENVELOPE_RELEASE: &str = "EnvelopeRelease";

// Indicator
const ENVELOPE_VALUE: &str = "EnvelopeValue";

/// Controls for audio parameters the audio input does not have. Messages to
/// them are accepted and ignored.
const UNSUPPORTED_CONTROLS: [&str; 2] = ["FilterCutoff", "Gain"];

pub fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    use ControlMessage::*;
    use StateChange::*;
    MONITOR_TOGGLE.map_trigger(map, || ToggleMonitor);
    map.add_unipolar(ENVELOPE_ATTACK, |v| {
        Set(EnvelopeAttack(envelope_edge_from_unipolar(v)))
    });
    map.add_unipolar(ENVELOPE_RELEASE, |v| {
        Set(EnvelopeRelease(envelope_edge_from_unipolar(v)))
    });
    RESET.map_trigger(map, || ResetParameters);
    for control in UNSUPPORTED_CONTROLS {
        map.add(control, |_| Ok(None));
    }
}

pub fn emit_osc_state_change<S>(sc: &StateChange, emitter: &S)
where
    S: crate::osc::EmitScopedOscMessage + ?Sized,
{
    match sc {
        StateChange::Monitor(v) => MONITOR_TOGGLE.send(*v, emitter),
        StateChange::EnvelopeAttack(v) => {
            emitter.emit_float(ENVELOPE_ATTACK, envelope_edge_to_unipolar(*v).val());
        }
        StateChange::EnvelopeRelease(v) => {
            emitter.emit_float(ENVELOPE_RELEASE, envelope_edge_to_unipolar(*v).val());
        }
        StateChange::EnvelopeValue(v) => {
            emitter.emit_float(ENVELOPE_VALUE, v.val());
        }
        // GUI-only audio parameters; no OSC feedback surface.
        StateChange::OutputSmoothing(_)
        | StateChange::NormFloorHalflife(_)
        | StateChange::NormCeilingHalflife(_) => {}
    }
}

/// Scaled to 1 to 128, in milliseconds.
/// Set using microseconds so we preserve full resolution on the input control.
/// This is janky.
fn envelope_edge_from_unipolar(v: UnipolarFloat) -> Duration {
    let millis = (v.val() * 127.) + 1.0;
    Duration::from_micros((millis * 1000.) as u64)
}

/// Clamp duration in integer milliseconds and scale into unipolar.
fn envelope_edge_to_unipolar(d: Duration) -> UnipolarFloat {
    UnipolarFloat::new(((d.as_micros() as f64 / 1000.) - 1.0) / 127.)
}
