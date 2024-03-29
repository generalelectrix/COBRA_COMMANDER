use rosc::OscMessage;

use super::generic::map_strobe;
use crate::fixture::generic::GenericStrobeStateChange;
use crate::fixture::swarmolon::{
    ControlMessage, DerbyColor, StateChange, Swarmolon, WhiteStrobeStateChange,
};
use crate::fixture::FixtureControlMessage;
use crate::osc::basic_controls::{button, Button};
use crate::osc::radio_button::EnumRadioButton;
use crate::osc::{get_bool, ControlMap, HandleStateChange, MapControls, RadioButton};
use crate::util::bipolar_fader_with_detent;

const GROUP: &str = "Swarmolon";

const STROBE_PROGRAM_SELECT: RadioButton = RadioButton {
    group: GROUP,
    control: "WhiteStrobeProgram",
    n: 10,
    x_primary_coordinate: false,
};

const RED_LASER_ON: Button = button(GROUP, "RedLaserOn");
const GREEN_LASER_ON: Button = button(GROUP, "GreenLaserOn");

impl EnumRadioButton for DerbyColor {}

impl MapControls for Swarmolon {
    fn map_controls(&self, map: &mut ControlMap<FixtureControlMessage>) {
        use ControlMessage::*;
        use FixtureControlMessage::Swarmolon;
        use StateChange::*;

        map.add_enum_handler(GROUP, "DerbyColor", get_bool, |c, v| {
            Swarmolon(Set(DerbyColor(c, v)))
        });
        map_strobe(map, GROUP, "DerbyStrobe", &wrap_derby_strobe);
        map.add_bipolar(GROUP, "DerbyRotation", |v| {
            Swarmolon(Set(DerbyRotation(bipolar_fader_with_detent(v))))
        });
        map_strobe(map, GROUP, "WhiteStrobe", &wrap_white_strobe);
        STROBE_PROGRAM_SELECT.map(map, |v| {
            Swarmolon(Set(WhiteStrobe(WhiteStrobeStateChange::Program(v))))
        });

        RED_LASER_ON.map_state(map, |v| Swarmolon(Set(RedLaserOn(v))));
        GREEN_LASER_ON.map_state(map, |v| Swarmolon(Set(GreenLaserOn(v))));
        map_strobe(map, GROUP, "LaserStrobe", &wrap_laser_strobe);
        map.add_bipolar(GROUP, "LaserRotation", |v| {
            Swarmolon(Set(LaserRotation(bipolar_fader_with_detent(v))))
        });

        // "Global" strobe rate control, for simpler one-fader control.
        // This is a bit of a hack, since it has no talkback channel.
        // This will need to be refactored if we want to use uniform talkback.
        map.add_unipolar(GROUP, "StrobeRate", |v| Swarmolon(StrobeRate(v)));
    }
}

fn wrap_derby_strobe(sc: GenericStrobeStateChange) -> FixtureControlMessage {
    FixtureControlMessage::Swarmolon(ControlMessage::Set(StateChange::DerbyStrobe(sc)))
}

fn wrap_white_strobe(sc: GenericStrobeStateChange) -> FixtureControlMessage {
    FixtureControlMessage::Swarmolon(ControlMessage::Set(StateChange::WhiteStrobe(
        WhiteStrobeStateChange::State(sc),
    )))
}

fn wrap_laser_strobe(sc: GenericStrobeStateChange) -> FixtureControlMessage {
    FixtureControlMessage::Swarmolon(ControlMessage::Set(StateChange::LaserStrobe(sc)))
}

impl HandleStateChange<StateChange> for Swarmolon {
    fn emit_state_change<S>(sc: StateChange, send: &mut S, _talkback: crate::osc::TalkbackMode)
    where
        S: FnMut(OscMessage),
    {
        use StateChange::*;
        #[allow(clippy::single_match)]
        match sc {
            WhiteStrobe(WhiteStrobeStateChange::Program(v)) => STROBE_PROGRAM_SELECT.set(v, send),
            _ => (),
        }
    }
}
