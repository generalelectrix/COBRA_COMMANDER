//! Device model for the Novation Launch Control XL.
use std::{cell::OnceCell, collections::HashMap};

use log::{debug, error};
use tunnels::{
    midi::{Event, EventType, Mapping, Output},
    midi_controls::MidiDevice,
};

use crate::{midi::Device, show::ChannelId};

/// Model of the Novation Launch Control XL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NovationLaunchControlXL {
    /// When interpreting channel control messages, offset the incoming channel
    /// by this amount.
    pub channel_offset: usize,
}

const FADER: u8 = 0;
const TOP_KNOB: u8 = 1;
const MIDDLE_KNOB: u8 = 2;
const BOTTOM_KNOB: u8 = 3;
const TRACK_FOCUS: u8 = 0;
const TRACK_CONTROL: u8 = 1;

impl NovationLaunchControlXL {
    pub const CHANNEL_COUNT: u8 = 8;

    pub fn device_name(&self) -> &str {
        "Launch Control XL"
    }

    /// Select factory template 0.
    pub fn init_midi<D: MidiDevice>(&self, out: &mut Output<D>) -> anyhow::Result<()> {
        debug!("Sending Launch Control XL sysex template select command (User 1).");
        out.send_raw(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x11, 0x77, 0x00, 0xF7])?;
        Ok(())
    }

    /// Determine the midi channel for the given show control channel.
    /// Return None if the show channel isn't mapped onto this device.
    pub fn midi_channel_for_control_channel(&self, channel: ChannelId) -> Option<u8> {
        let midi_channel = channel.inner() as isize - self.channel_offset as isize;
        (midi_channel >= 0 && midi_channel < Self::CHANNEL_COUNT as isize)
            .then_some(midi_channel as u8)
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<LaunchControlXLControlEvent> {
        use LaunchControlXLChannelButton::*;
        use LaunchControlXLChannelControlEvent::*;
        use LaunchControlXLControlEvent::*;
        let event = match event.mapping.event_type {
            EventType::ControlChange => Some(Channel {
                channel: event.mapping.channel,
                event: match event.mapping.control {
                    FADER => Fader(event.value),
                    TOP_KNOB => Knob {
                        row: 0,
                        val: event.value,
                    },
                    MIDDLE_KNOB => Knob {
                        row: 1,
                        val: event.value,
                    },
                    BOTTOM_KNOB => Knob {
                        row: 2,
                        val: event.value,
                    },
                    _ => {
                        return None;
                    }
                },
            }),
            EventType::NoteOn if event.mapping.channel == 8 => {
                use LaunchControlXLSideButton::*;
                let button = match event.mapping.control {
                    0 => Up,
                    1 => Down,
                    2 => Left,
                    3 => Right,
                    4 => Device,
                    5 => Mute,
                    6 => Solo,
                    7 => Record,
                    _ => {
                        return None;
                    }
                };
                Some(SideButton(button))
            }
            EventType::NoteOn => match event.mapping.control {
                TRACK_FOCUS => Some(Channel {
                    channel: event.mapping.channel,
                    event: Button(TrackFocus),
                }),
                TRACK_CONTROL => Some(Channel {
                    channel: event.mapping.channel,
                    event: Button(TrackControl),
                }),
                _ => None,
            },
            _ => None,
        };
        println!("{event:?}");
        event
    }

    /// Process a state change and emit midi.
    pub fn emit(&self, sc: LaunchControlXLStateChange, output: &mut Output<Device>) {
        use LaunchControlXLChannelButton::*;
        use LaunchControlXLStateChange::*;
        match sc {
            ChannelButtonRadio {
                channel,
                button,
                state,
            } => {
                let control = match button {
                    TrackFocus => TRACK_FOCUS,
                    TrackControl => TRACK_CONTROL,
                };
                for c in 0..8 {
                    if let Err(err) = output.send(Event {
                        mapping: Mapping {
                            event_type: EventType::NoteOn,
                            channel: c,
                            control,
                        },
                        value: if Some(c) == channel {
                            state.as_byte()
                        } else {
                            0
                        },
                    }) {
                        error!("midi send error for APC20: {err}");
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLControlEvent {
    Channel {
        channel: u8,
        event: LaunchControlXLChannelControlEvent,
    },
    SideButton(LaunchControlXLSideButton),
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLChannelControlEvent {
    Fader(u8),
    Knob {
        /// Numbered from the top.
        row: u8,
        val: u8,
    },
    Button(LaunchControlXLChannelButton),
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLChannelButton {
    TrackFocus,   // top button
    TrackControl, // bottom button
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLSideButton {
    Up,
    Down,
    Left,
    Right,
    Device,
    Mute,
    Solo,
    Record,
}

#[derive(Clone, Copy)]
pub enum LaunchControlXLStateChange {
    // SingleChannelButton {
    //     channel: u8,
    //     button: LaunchControlXLChannelButtonType,
    //     on: bool, // TODO: model blinking
    // },
    /// Set the specified channel on, all others off
    /// If channel is None, turn all buttons off.
    ChannelButtonRadio {
        channel: Option<u8>,
        button: LaunchControlXLChannelButton,
        state: LedState,
    },
    // TODO: knob LED state
}

#[derive(Clone, Copy)]
pub struct LedState {
    red: u8,   // [0, 3]
    green: u8, // [0, 3]
}

impl LedState {
    pub const YELLOW: Self = Self { red: 3, green: 3 };

    fn as_byte(self) -> u8 {
        0b1100 + self.red + (self.green << 4)
    }
}

#[test]
fn test_led_state_as_byte() {
    let s = LedState {
        red: 0b11,
        green: 0b10,
    };
    assert_eq!(0b0101111, s.as_byte());
}