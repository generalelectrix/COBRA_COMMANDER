use rosc::OscMessage;

use crate::animation::StateChange;
use crate::fixture::animation_target::{AnimationTarget, AnimationTargetIndex};
use crate::fixture::wizard_extreme::AnimationTarget as WizardExtremeAnimationTarget;
use crate::fixture::{FixtureControlMessage, N_ANIM};
use crate::osc::radio_button::EnumRadioButton;
use crate::osc::{ignore_payload, HandleStateChange};
use crate::osc::{ControlMap, MapControls, RadioButton};

use super::AnimationControls;

const N_ANIM_TARGET: usize = 11;

const GROUP: &str = "Animation";
const TARGET: &str = "Target";

const ANIMATION_SELECT: RadioButton = RadioButton {
    group: GROUP,
    control: "Select",
    n: N_ANIM,
    x_primary_coordinate: false,
};

const ANIMATION_TARGET_SELECT: RadioButton = RadioButton {
    group: GROUP,
    control: TARGET,
    n: N_ANIM_TARGET as usize,
    x_primary_coordinate: false,
};
pub struct AnimationTargetControls;

impl MapControls for AnimationTargetControls {
    fn map_controls(&self, map: &mut ControlMap<FixtureControlMessage>) {
        use FixtureControlMessage::{AnimationSelect, AnimationTarget};

        map.add_radio_button_array(ANIMATION_TARGET_SELECT, AnimationTarget);
        map.add_radio_button_array(ANIMATION_SELECT, AnimationSelect);
    }
}

impl HandleStateChange<StateChange> for AnimationTargetControls {
    fn emit_state_change<S>(sc: StateChange, send: &mut S)
    where
        S: FnMut(OscMessage),
    {
        match sc {
            StateChange::Animation(msg) => AnimationControls::emit_state_change(msg, send),
            StateChange::Select(msg) => ANIMATION_SELECT.set(msg, send),
            StateChange::Target(msg) => ANIMATION_TARGET_SELECT.set(msg, send),
        }
    }
}
