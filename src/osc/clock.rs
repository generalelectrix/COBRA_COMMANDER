//! OSC control mappings for the tunnels clock system.

use tunnels::clock_bank::StateChange;

pub const GROUP: &str = "Clock";

pub fn emit_osc_state_change<S>(_sc: &StateChange, _emitter: &S)
where
    S: crate::osc::EmitScopedOscMessage + ?Sized,
{
    // TODO: implement clock controls
}
