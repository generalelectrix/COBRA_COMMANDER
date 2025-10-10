//! Device model for the Behringer CMD MM-1 fader wing.
use clap::error;
use log::{debug, error, warn};
use strum_macros::Display;
use tunnels::{
    clock_bank::ClockIdxExt,
    midi::{Event, EventType, Mapping, Output},
    midi_controls::{bipolar_from_midi, unipolar_from_midi, MidiDevice},
};

use tunnels::clock::{ControlMessage as ClockControlMessage, StateChange as ClockStateChange};
use tunnels::clock_bank::{
    ControlMessage as ClockBankControlMessage, StateChange as ClockBankStateChange,
};

use crate::{
    channel::KnobValue,
    midi::{Device, MidiHandler},
    show::{ChannelId, ShowControlMessage},
};

/// Model of the Behringer CMD-MM1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehringerCmdMM1 {}

const MIDI_CHANNEL: u8 = 4;

impl BehringerCmdMM1 {
    pub const CHANNEL_COUNT: u8 = 4;

    pub fn device_name(&self) -> &str {
        "CMD MM-1"
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<CmdMM1ControlEvent> {
        use CmdMM1ChannelButton::*;
        use CmdMM1ChannelControlEvent::*;
        use CmdMM1ControlEvent::*;
        // All controls are on channel 4
        if event.mapping.channel != MIDI_CHANNEL {
            return None;
        }
        let control = event.mapping.control;
        match event.mapping.event_type {
            EventType::ControlChange => {
                match control {
                    6..=21 => {
                        // Figure out which row and channel.
                        let index = control - 6;
                        let row = index / 4;
                        let channel = index % 4;
                        Some(Channel {
                            channel,
                            event: Knob {
                                row,
                                val: event.value,
                            },
                        })
                    }
                    48..51 => {
                        let channel = control - 48;
                        Some(Channel {
                            channel,
                            event: Fader(event.value),
                        })
                    }
                    // TODO: handle other knobs
                    _ => None,
                }
            }
            EventType::NoteOn => match control {
                48..=51 => Some(Channel {
                    channel: control - 48,
                    event: Button(Cue),
                }),
                19 | 23 | 27 | 31 => Some(Channel {
                    channel: (control - 19) / 4,
                    event: Button(One),
                }),
                20 | 24 | 28 | 32 => Some(Channel {
                    channel: (control - 20) / 4,
                    event: Button(Two),
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// Set a button LED on or off.
    pub fn set_led(
        &self,
        channel: usize,
        button: CmdMM1ChannelButton,
        state: bool,
        output: &mut Output<Device>,
    ) {
        if channel >= Self::CHANNEL_COUNT as usize {
            warn!("CMD MM-1 channel {channel} out of range for LED state update");
            return;
        }
        let control = match button {
            CmdMM1ChannelButton::Cue => 48 + channel as u8,
            CmdMM1ChannelButton::One => 19 + (channel as u8 * 4),
            CmdMM1ChannelButton::Two => 20 + (channel as u8 + 4),
        };
        if let Err(err) = output.send(Event {
            mapping: Mapping {
                event_type: EventType::NoteOn,
                channel: MIDI_CHANNEL,
                control,
            },
            value: state as u8,
        }) {
            error!("MIDI send error setting LED state {channel}({button}) to {state}: {err}.");
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CmdMM1ControlEvent {
    Channel {
        channel: u8,
        event: CmdMM1ChannelControlEvent,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum CmdMM1ChannelControlEvent {
    Fader(u8),
    Knob {
        /// Numbered from the top.
        row: u8,
        val: u8,
    },
    Button(CmdMM1ChannelButton),
}

#[derive(Clone, Copy, Debug, Display)]
pub enum CmdMM1ChannelButton {
    One, // "1" button
    Two, // "2" button
    Cue, // "CUE" button
}

impl MidiHandler for BehringerCmdMM1 {
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage> {
        use CmdMM1ChannelButton::*;
        use CmdMM1ChannelControlEvent::*;
        use CmdMM1ControlEvent::*;
        Some(ShowControlMessage::Clock(match self.parse(event)? {
            Channel { channel, event } => ClockBankControlMessage {
                channel: ClockIdxExt(channel as usize),
                msg: match event {
                    Fader(val) => ClockControlMessage::Set(ClockStateChange::SubmasterLevel(
                        unipolar_from_midi(val),
                    )),
                    Knob { row, val } => match row {
                        0 => {
                            ClockControlMessage::Set(ClockStateChange::Rate(bipolar_from_midi(val)))
                        }
                        1 => ClockControlMessage::Set(ClockStateChange::RateFine(
                            bipolar_from_midi(val),
                        )),
                        _ => {
                            return None;
                        }
                    },
                    Button(b) => match b {
                        Cue => ClockControlMessage::Tap,
                        One => ClockControlMessage::ToggleOneShot,
                        Two => ClockControlMessage::ToggleRetrigger,
                    },
                },
            },
        }))
    }

    fn emit_clock_control(&self, msg: &ClockBankStateChange, output: &mut Output<Device>) {
        let channel: usize = msg.channel.into();
        match msg.change {
            ClockStateChange::OneShot(v) => {
                self.set_led(channel, CmdMM1ChannelButton::One, v, output)
            }
            ClockStateChange::Retrigger(v) => {
                self.set_led(channel, CmdMM1ChannelButton::Two, v, output)
            }
            ClockStateChange::Ticked(v) => {
                self.set_led(channel, CmdMM1ChannelButton::Cue, v, output)
            }
            ClockStateChange::Rate(_)
            | ClockStateChange::RateFine(_)
            | ClockStateChange::SubmasterLevel(_)
            | ClockStateChange::UseAudioSize(_)
            | ClockStateChange::UseAudioSpeed(_) => (),
        }
    }
}
