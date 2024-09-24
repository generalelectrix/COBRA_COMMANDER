use crate::fixture::dimmer::{Dimmer, StateChange};
use crate::fixture::ControlMessagePayload;
use crate::fixture::PatchAnimatedFixture;
use crate::osc::{ControlMap, HandleStateChange, MapControls};

const GROUP: &str = "Dimmer";

impl MapControls for Dimmer {
    fn map_controls(&self, map: &mut ControlMap<ControlMessagePayload>) {
        map.add_unipolar(GROUP, "Level", ControlMessagePayload::fixture);
    }

    fn fixture_type_aliases(&self) -> Vec<(String, crate::fixture::FixtureType)> {
        vec![(GROUP.to_string(), Self::NAME)]
    }
}

impl HandleStateChange<StateChange> for Dimmer {
    fn emit_state_change<S>(_sc: StateChange, _send: &mut S, _talkback: crate::osc::TalkbackMode)
    where
        S: crate::osc::EmitControlMessage,
    {
    }
}
