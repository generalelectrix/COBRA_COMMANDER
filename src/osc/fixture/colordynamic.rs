use rosc::OscMessage;

use super::generic::map_strobe;
use crate::fixture::colordynamic::{Colordynamic, StateChange};
use crate::fixture::generic::GenericStrobeStateChange;
use crate::fixture::FixtureControlMessage;

use crate::osc::basic_controls::{button, Button};
use crate::osc::HandleStateChange;
use crate::osc::{ControlMap, MapControls};
use crate::util::bipolar_fader_with_detent;

const GROUP: &str = "Colordynamic";

const SHUTTER_OPEN: Button = button(GROUP, "ShutterOpen");
const COLOR_ROTATION_ON: Button = button(GROUP, "ColorRotationOn");

impl MapControls for Colordynamic {
    fn map_controls(&self, map: &mut ControlMap<FixtureControlMessage>) {
        use FixtureControlMessage::Colordynamic;
        use StateChange::*;
        SHUTTER_OPEN.map_state(map, |v| Colordynamic(ShutterOpen(v)));
        map_strobe(map, GROUP, "Strobe", &wrap_strobe);

        COLOR_ROTATION_ON.map_state(map, |v| Colordynamic(ColorRotationOn(v)));
        map.add_unipolar(GROUP, "ColorRotationSpeed", |v| {
            Colordynamic(ColorRotationSpeed(v))
        });
        map.add_unipolar(GROUP, "ColorPosition", |v| Colordynamic(ColorPosition(v)));
        map.add_bipolar(GROUP, "FiberRotation", |v| {
            Colordynamic(FiberRotation(bipolar_fader_with_detent(v)))
        });
    }
}

fn wrap_strobe(sc: GenericStrobeStateChange) -> FixtureControlMessage {
    FixtureControlMessage::Colordynamic(StateChange::Strobe(sc))
}

impl HandleStateChange<StateChange> for Colordynamic {
    fn emit_state_change<S>(_sc: StateChange, _send: &mut S, _talkback: crate::osc::TalkbackMode)
    where
        S: FnMut(OscMessage),
    {
        // FIXME no talkback
    }
}
