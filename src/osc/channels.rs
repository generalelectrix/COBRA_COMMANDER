use crate::channel::{ChannelControlMessage, ChannelStateChange, Channels, KnobIndex};
use crate::channel::{ControlMessage, StateChange};

use crate::osc::prelude::ButtonArray;
use crate::osc::{GroupControlMap, RadioButton};

use super::label_array::LabelArray;
use super::unipolar_array::{unipolar_array, UnipolarArray};
use anyhow::{anyhow, Context};

const N_CHANNELS: usize = 8;

pub(crate) const GROUP: &str = "Show";

impl Channels {
    pub fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
        CHANNEL_SELECT.map(map, ControlMessage::SelectChannel);
        CHANNEL_STROBE.map(map, |channel_id| ControlMessage::Control {
            channel_id: Some(channel_id),
            msg: ChannelControlMessage::ToggleStrobe,
        });
        CHANNEL_FADERS.map(map, |channel_id, level| {
            Ok(ControlMessage::Control {
                channel_id: Some(channel_id),
                msg: ChannelControlMessage::Level(level),
            })
        });
        map.add("ChannelKnob", |msg| {
            let index = msg
                .addr_payload()
                .split('/')
                .skip(1)
                .take(1)
                .next()
                .ok_or_else(|| anyhow!("channel knob index missing for {msg:?}"))?
                .parse::<KnobIndex>()
                .with_context(|| format!("handling message {msg:?}"))?;
            let val = msg.get_unipolar()?;

            Ok(Some(ControlMessage::Control {
                channel_id: None,
                msg: ChannelControlMessage::Knob {
                    index,
                    value: crate::channel::KnobValue::Unipolar(val),
                },
            }))
        });
    }

    pub fn emit_osc_state_change<S>(sc: StateChange, send: &S)
    where
        S: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        match sc {
            StateChange::SelectChannel(channel_id) => {
                CHANNEL_SELECT.set(channel_id.into(), true, send)
            }
            StateChange::ChannelLabels(labels) => CHANNEL_LABELS.set(labels.into_iter(), send),
            StateChange::State { channel_id, msg } => match msg {
                ChannelStateChange::Level(l) => CHANNEL_FADERS.set(channel_id.into(), l, send),
                ChannelStateChange::Knob { index, value } => {
                    send.emit_float(&format!("ChannelKnob/{index}"), value.as_unipolar().val());
                }
                ChannelStateChange::Strobe(v) => {
                    CHANNEL_STROBE.set(channel_id.into(), v, send);
                }
            },
        }
    }
}

const CHANNEL_SELECT: RadioButton = RadioButton {
    control: "Channel",
    n: N_CHANNELS,
    x_primary_coordinate: false,
};

const CHANNEL_STROBE: ButtonArray = ButtonArray {
    control: "ChannelStrobe",
};

const CHANNEL_LABELS: LabelArray = LabelArray {
    control: "ChannelLabel",
    n: N_CHANNELS,
    empty_label: "",
};

const CHANNEL_FADERS: UnipolarArray = unipolar_array("ChannelLevel");
