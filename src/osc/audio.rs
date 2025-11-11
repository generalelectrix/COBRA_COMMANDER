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
const FILTER_CUTOFF: &str = "FilterCutoff";
const ENVELOPE_ATTACK: &str = "EnvelopeAttack";
const ENVELOPE_RELEASE: &str = "EnvelopeRelease";
const GAIN: &str = "Gain";

// Indicator
const IS_CLIPPING: &str = "IsClipping";
const ENVELOPE_VALUE: &str = "EnvelopeValue";

pub fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    use ControlMessage::*;
    use StateChange::*;
    MONITOR_TOGGLE.map_trigger(map, || ToggleMonitor);
    map.add_unipolar(FILTER_CUTOFF, |v| {
        Set(FilterCutoff(filter_from_unipolar(v)))
    });
    map.add_unipolar(ENVELOPE_ATTACK, |v| {
        Set(EnvelopeAttack(envelope_edge_from_unipolar(v)))
    });
    map.add_unipolar(ENVELOPE_RELEASE, |v| {
        Set(EnvelopeRelease(envelope_edge_from_unipolar(v)))
    });
    map.add_unipolar(GAIN, |v| Set(Gain(gain_from_unipolar(v))));
    RESET.map_trigger(map, || ResetParameters);
}

pub fn emit_osc_state_change<S>(sc: &StateChange, emitter: &S)
where
    S: crate::osc::EmitScopedOscMessage + ?Sized,
{
    match sc {
        StateChange::Monitor(v) => MONITOR_TOGGLE.send(*v, emitter),
        StateChange::FilterCutoff(v) => {
            emitter.emit_float(FILTER_CUTOFF, filter_to_unipolar(*v).val())
        }
        StateChange::EnvelopeAttack(v) => {
            emitter.emit_float(ENVELOPE_ATTACK, envelope_edge_to_unipolar(*v).val());
        }
        StateChange::EnvelopeRelease(v) => {
            emitter.emit_float(ENVELOPE_RELEASE, envelope_edge_to_unipolar(*v).val());
        }
        StateChange::Gain(v) => {
            emitter.emit_float(GAIN, gain_to_unipolar(*v).val());
        }
        StateChange::IsClipping(v) => {
            emitter.emit_float(IS_CLIPPING, if *v { 1.0 } else { 0.0 });
        }
        StateChange::EnvelopeValue(v) => {
            emitter.emit_float(ENVELOPE_VALUE, v.val());
        }
    }
}

// FIXME: copy-pasta consts from tunnels
// Crude filter control - linear, roughly 1kHz range, "0" is 40 Hz.
// FIXME: make this logarithmic

const FILTER_LOWER_BOUND: f64 = 40.;
const FILTER_SCALE: f64 = 1000.;

fn filter_from_unipolar(v: UnipolarFloat) -> f32 {
    (v.val() * FILTER_SCALE + FILTER_LOWER_BOUND) as f32
}

fn filter_to_unipolar(f: f32) -> UnipolarFloat {
    UnipolarFloat::new(((f as f64) - FILTER_LOWER_BOUND) / FILTER_SCALE)
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

// Set gain as a unipolar knob, scaled by 20, interpreted as dB.
fn gain_from_unipolar(v: UnipolarFloat) -> f64 {
    let gain_db = 10. * v.val();
    (10_f64).powf(gain_db / 20.)
}

fn gain_to_unipolar(g: f64) -> UnipolarFloat {
    let gain_db = 20. * g.log10();
    UnipolarFloat::new(gain_db / 10.)
}
