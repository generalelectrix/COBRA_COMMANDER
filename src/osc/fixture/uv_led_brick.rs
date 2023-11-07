use crate::fixture::uv_led_brick::{StateChange, UvLedBrick};
use crate::fixture::FixtureControlMessage;
use crate::osc::{ControlMap, HandleStateChange, MapControls};

const GROUP: &str = "UvLedBrick";

impl MapControls for UvLedBrick {
    fn map_controls(&self, map: &mut ControlMap<FixtureControlMessage>) {
        map.add_unipolar(GROUP, "Level", FixtureControlMessage::UvLedBrick);
    }
}

impl HandleStateChange<StateChange> for UvLedBrick {}
