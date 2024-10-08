use crate::fixture::generic::GenericStrobeStateChange;
use crate::fixture::lumasphere::StrobeStateChange;
use crate::fixture::lumasphere::{Lumasphere, StateChange};
use crate::fixture::ControlMessagePayload;
use crate::fixture::PatchFixture;
use crate::osc::basic_controls::{button, Button};
use crate::osc::{ControlMap, HandleOscStateChange, MapControls};
use crate::util::bipolar_fader_with_detent;
use crate::util::unipolar_fader_with_detent;

const GROUP: &str = "Lumasphere";

const BALL_START: Button = button(GROUP, "ball_start");
const COLOR_START: Button = button(GROUP, "color_start");

impl MapControls for Lumasphere {
    fn map_controls(&self, map: &mut ControlMap<ControlMessagePayload>) {
        use StateChange::*;
        map.add_unipolar(GROUP, "lamp_1_intensity", |v| {
            ControlMessagePayload::fixture(Lamp1Intensity(unipolar_fader_with_detent(v)))
        });
        map.add_unipolar(GROUP, "lamp_2_intensity", |v| {
            ControlMessagePayload::fixture(Lamp2Intensity(unipolar_fader_with_detent(v)))
        });

        map.add_bipolar(GROUP, "ball_rotation", |v| {
            ControlMessagePayload::fixture(BallRotation(bipolar_fader_with_detent(v)))
        });
        BALL_START.map_state(map, |v| ControlMessagePayload::fixture(BallStart(v)));

        map.add_unipolar(GROUP, "color_rotation", |v| {
            ControlMessagePayload::fixture(ColorRotation(unipolar_fader_with_detent(v)))
        });
        COLOR_START.map_state(map, |v| ControlMessagePayload::fixture(ColorStart(v)));
        map_strobe(map, 1, |inner| {
            ControlMessagePayload::fixture(Strobe1(inner))
        });
        map_strobe(map, 2, |inner| {
            ControlMessagePayload::fixture(Strobe2(inner))
        });
    }

    fn fixture_type_aliases(&self) -> Vec<(String, crate::fixture::FixtureType)> {
        vec![(GROUP.to_string(), Self::NAME)]
    }
}

fn map_strobe<W>(map: &mut ControlMap<ControlMessagePayload>, index: u8, wrap: W)
where
    W: Fn(StrobeStateChange) -> ControlMessagePayload + 'static + Copy,
{
    use GenericStrobeStateChange::*;
    use StrobeStateChange::*;
    map.add_bool(GROUP, &format!("strobe_{}_state", index), move |v| {
        wrap(State(On(v)))
    });
    map.add_unipolar(GROUP, &format!("strobe_{}_rate", index), move |v| {
        wrap(State(Rate(v)))
    });
    map.add_unipolar(GROUP, &format!("strobe_{}_intensity", index), move |v| {
        wrap(Intensity(v))
    });
}

impl HandleOscStateChange<StateChange> for Lumasphere {}
