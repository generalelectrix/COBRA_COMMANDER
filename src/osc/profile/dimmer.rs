use crate::fixture::dimmer::{Dimmer, StateChange};
use crate::fixture::ControlMessagePayload;
use crate::fixture::PatchAnimatedFixture;
use crate::osc::{ControlMap, HandleOscStateChange, MapControls};

const GROUP: &str = "Dimmer";

impl MapControls for Dimmer {
    fn map_controls(&self, map: &mut ControlMap<ControlMessagePayload>) {
        map.add_unipolar(GROUP, "Level", ControlMessagePayload::fixture);
    }

    fn fixture_type_aliases(&self) -> Vec<(String, crate::fixture::FixtureType)> {
        vec![(GROUP.to_string(), Self::NAME)]
    }
}

impl HandleOscStateChange<StateChange> for Dimmer {
    fn emit_osc_state_change<S>(_sc: StateChange, _send: &S)
    where
        S: crate::osc::EmitOscMessage + ?Sized,
    {
    }
}