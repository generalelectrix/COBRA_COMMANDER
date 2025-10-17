//! Mappings between show control events and midi device-specific actions.
//!
use tunnels::{midi::Output, midi_controls::unipolar_from_midi};

use super::{
    device::{
        apc20::{
            AkaiApc20, Apc20ChannelButtonType, Apc20ChannelControlEvent, Apc20ControlEvent,
            Apc20StateChange,
        },
        launch_control_xl::{
            LaunchControlXLChannelButton, LaunchControlXLChannelControlEvent,
            LaunchControlXLChannelStateChange, LaunchControlXLControlEvent,
            LaunchControlXLStateChange, LedState, NovationLaunchControlXL,
        },
    },
    MidiHandler,
};
use crate::{
    channel::{
        ChannelControlMessage as ScopedChannelControlMessage,
        ChannelStateChange as SpecificChannelStateChange, ControlMessage as ChannelControlMessage,
        KnobValue, StateChange as ChannelStateChange,
    },
    show::ShowControlMessage,
};

impl MidiHandler for AkaiApc20 {
    fn interpret(&self, event: &tunnels::midi::Event) -> Option<ShowControlMessage> {
        use Apc20ChannelButtonType::*;
        use Apc20ChannelControlEvent::*;
        use Apc20ControlEvent::*;
        Some(ShowControlMessage::Channel(match self.parse(event)? {
            Channel { channel, event } => match event {
                Fader(val) => ChannelControlMessage::Control {
                    channel_id: Some(channel as usize + self.channel_offset),
                    msg: ScopedChannelControlMessage::Level(unipolar_from_midi(val)),
                },
                Button(TrackSelect) => {
                    ChannelControlMessage::SelectChannel(channel as usize + self.channel_offset)
                }
            },
        }))
    }

    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut Output) {
        if let ChannelStateChange::SelectChannel(channel) = msg {
            let midi_channel = channel.inner() as isize - self.channel_offset as isize;
            let midi_channel = (midi_channel >= 0 && midi_channel < Self::CHANNEL_COUNT as isize)
                .then_some(midi_channel as u8);
            self.emit(
                Apc20StateChange::ChannelButtonRadio {
                    channel: midi_channel,
                    button: Apc20ChannelButtonType::TrackSelect,
                },
                output,
            );
        }
    }
}

impl MidiHandler for NovationLaunchControlXL {
    fn interpret(&self, event: &tunnels::midi::Event) -> Option<ShowControlMessage> {
        use LaunchControlXLChannelButton::*;
        use LaunchControlXLChannelControlEvent::*;
        use LaunchControlXLControlEvent::*;
        Some(ShowControlMessage::Channel(match self.parse(event)? {
            Channel { channel, event } => match event {
                Fader(val) => ChannelControlMessage::Control {
                    channel_id: Some(channel as usize + self.channel_offset),
                    msg: ScopedChannelControlMessage::Level(unipolar_from_midi(val)),
                },
                Knob { row, val } => ChannelControlMessage::Control {
                    channel_id: Some(channel as usize + self.channel_offset),
                    msg: ScopedChannelControlMessage::Knob {
                        index: row, // TODO: these are numbered top to bottom, do we want bottom to top?
                        value: KnobValue::Unipolar(unipolar_from_midi(val)),
                    },
                },
                Button(TrackFocus) => {
                    ChannelControlMessage::SelectChannel(channel as usize + self.channel_offset)
                }
                Button(TrackControl) => ChannelControlMessage::Control {
                    channel_id: Some(channel as usize + self.channel_offset),
                    msg: ScopedChannelControlMessage::ToggleStrobe,
                },
            },
            SideButton(_) => {
                return None;
            }
        }))
    }

    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut Output) {
        match msg {
            ChannelStateChange::SelectChannel(channel) => {
                let midi_channel = self.midi_channel_for_control_channel(*channel);
                self.emit(
                    LaunchControlXLStateChange::ChannelButtonRadio {
                        channel: midi_channel,
                        button: LaunchControlXLChannelButton::TrackFocus,
                        state: LedState::YELLOW,
                    },
                    output,
                );
            }
            ChannelStateChange::State { channel_id, msg } => {
                let Some(channel) = self.midi_channel_for_control_channel(*channel_id) else {
                    return;
                };
                match msg {
                    SpecificChannelStateChange::Knob { index, value } => self.emit(
                        LaunchControlXLStateChange::Channel {
                            channel,
                            state: LaunchControlXLChannelStateChange::Knob {
                                row: *index,
                                state: LedState::from_knob_value(value),
                            },
                        },
                        output,
                    ),
                    SpecificChannelStateChange::Level(_) => (),
                    SpecificChannelStateChange::Strobe(v) => {
                        self.emit(
                            LaunchControlXLStateChange::ChannelButton {
                                channel,
                                button: LaunchControlXLChannelButton::TrackControl,
                                state: if *v { LedState::RED } else { LedState::OFF },
                            },
                            output,
                        );
                    }
                }
            }
            ChannelStateChange::ChannelLabels(_) => (),
        }
    }
}
