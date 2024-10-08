use super::generic::map_strobe;
use crate::fixture::generic::GenericStrobeStateChange;
use crate::fixture::rush_wizard::{Color, RushWizard, StateChange};
use crate::fixture::ControlMessagePayload;
use crate::fixture::PatchFixture;
use crate::osc::basic_controls::{button, Button};
use crate::osc::radio_button::EnumRadioButton;
use crate::osc::{ignore_payload, HandleOscStateChange};
use crate::osc::{ControlMap, MapControls, RadioButton};
use crate::util::bipolar_fader_with_detent;

const GROUP: &str = "RushWizard";
const COLOR: &str = "Color";

const GOBO_SELECT: RadioButton = RadioButton {
    group: GROUP,
    control: "Gobo",
    n: 16,
    x_primary_coordinate: false,
};

const TWINKLE: Button = button(GROUP, "Twinkle");

impl EnumRadioButton for Color {}

impl MapControls for RushWizard {
    fn map_controls(&self, map: &mut ControlMap<ControlMessagePayload>) {
        use StateChange::*;
        map.add_unipolar(GROUP, "Dimmer", |v| {
            ControlMessagePayload::fixture(Dimmer(v))
        });
        map_strobe(map, GROUP, "Strobe", &wrap_strobe);
        map.add_enum_handler(GROUP, COLOR, ignore_payload, |c, _| {
            ControlMessagePayload::fixture(Color(c))
        });
        TWINKLE.map_state(map, |v| ControlMessagePayload::fixture(Twinkle(v)));
        map.add_unipolar(GROUP, "TwinkleSpeed", |v| {
            ControlMessagePayload::fixture(TwinkleSpeed(v))
        });
        GOBO_SELECT.map(map, |v| ControlMessagePayload::fixture(Gobo(v)));
        map.add_bipolar(GROUP, "DrumRotation", |v| {
            ControlMessagePayload::fixture(DrumRotation(bipolar_fader_with_detent(v)))
        });
        map.add_bipolar(GROUP, "DrumSwivel", |v| {
            ControlMessagePayload::fixture(DrumSwivel(v))
        });
        map.add_bipolar(GROUP, "ReflectorRotation", |v| {
            ControlMessagePayload::fixture(ReflectorRotation(bipolar_fader_with_detent(v)))
        });
    }

    fn fixture_type_aliases(&self) -> Vec<(String, crate::fixture::FixtureType)> {
        vec![(GROUP.to_string(), Self::NAME)]
    }
}

fn wrap_strobe(sc: GenericStrobeStateChange) -> ControlMessagePayload {
    ControlMessagePayload::fixture(StateChange::Strobe(sc))
}

impl HandleOscStateChange<StateChange> for RushWizard {
    fn emit_osc_state_change<S>(sc: StateChange, send: &S)
    where
        S: crate::osc::EmitOscMessage + ?Sized,
    {
        match sc {
            StateChange::Color(c) => {
                c.set(GROUP, COLOR, send);
            }
            StateChange::Gobo(v) => GOBO_SELECT.set(v, send),
            _ => (),
        }
    }
}
