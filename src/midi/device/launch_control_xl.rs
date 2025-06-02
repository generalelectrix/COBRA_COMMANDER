//! Device model for the Novation Launch Control XL.
use log::{debug, error};
use tunnels::{
    midi::{Event, EventType, Output},
    midi_controls::MidiDevice,
};

use crate::{channel::KnobValue, midi::Device, show::ChannelId};

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
const TRACK_FOCUS: u8 = 1;
const TRACK_CONTROL: u8 = 2;

const TEMPLATE_ID: u8 = 0x00;

fn set_led<D: MidiDevice>(index: u8, state: LedState, out: &mut Output<D>) {
    if let Err(err) = out.send_raw(&[
        0xF0,
        0x00,
        0x20,
        0x29,
        0x02,
        0x11,
        0x78,
        TEMPLATE_ID,
        index,
        state.as_byte(),
        0xF7,
    ]) {
        error!("MIDI send error setting LED index {index}: {err}.");
    }
}

impl NovationLaunchControlXL {
    pub const CHANNEL_COUNT: u8 = 8;

    pub fn device_name(&self) -> &str {
        "Launch Control XL"
    }

    /// Select factory template 0.
    pub fn init_midi<D: MidiDevice>(&self, out: &mut Output<D>) -> anyhow::Result<()> {
        debug!("Sending Launch Control XL sysex template select command (User 1).");
        out.send_raw(&[0xF0, 0x00, 0x20, 0x29, 0x02, 0x11, 0x77, TEMPLATE_ID, 0xF7])?;
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
        match event.mapping.event_type {
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
                    12 => Up,
                    13 => Down,
                    14 => Left,
                    15 => Right,
                    16 => Device,
                    17 => Mute,
                    18 => Solo,
                    19 => Record,
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
        }
    }

    /// Process a state change and emit midi.
    pub fn emit(&self, sc: LaunchControlXLStateChange, output: &mut Output<Device>) {
        use LaunchControlXLChannelStateChange::*;
        use LaunchControlXLSideButton::*;
        use LaunchControlXLStateChange::*;

        match sc {
            Channel { channel, state } => match state {
                Knob { row, state } => {
                    if row > 2 {
                        error!("Launch Control XL knob index {row} out of range.");
                        return;
                    }
                    set_led((row * 8) + channel, state, output);
                }
                Button { button, state } => {
                    set_led(button.sysex_set_led_offset() + channel, state, output);
                }
            },
            ChannelButtonRadio {
                channel,
                button,
                state,
            } => {
                let start_index = button.sysex_set_led_offset();
                for c in 0..8 {
                    set_led(
                        start_index + c,
                        if Some(c) == channel {
                            state
                        } else {
                            LedState::OFF
                        },
                        output,
                    );
                }
            }
            SideButton { button, state } => set_led(
                match button {
                    Up => 44,
                    Down => 45,
                    Left => 46,
                    Right => 47,
                    Device => 40,
                    Mute => 41,
                    Solo => 42,
                    Record => 43,
                },
                state,
                output,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLControlEvent {
    Channel {
        channel: u8,
        event: LaunchControlXLChannelControlEvent,
    },
    #[allow(unused)]
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

impl LaunchControlXLChannelButton {
    pub fn sysex_set_led_offset(&self) -> u8 {
        match self {
            Self::TrackFocus => 24,
            Self::TrackControl => 32,
        }
    }
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

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLStateChange {
    Channel {
        channel: u8,
        state: LaunchControlXLChannelStateChange,
    },
    /// Set the specified channel on, all others off
    /// If channel is None, turn all buttons off.
    ChannelButtonRadio {
        channel: Option<u8>,
        button: LaunchControlXLChannelButton,
        state: LedState,
    },
    #[allow(unused)]
    SideButton {
        button: LaunchControlXLSideButton,
        state: LedState,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum LaunchControlXLChannelStateChange {
    Knob {
        row: u8,
        state: LedState,
    },
    #[allow(unused)]
    Button {
        button: LaunchControlXLChannelButton,
        state: LedState,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct LedState {
    red: u8,   // [0, 3]
    green: u8, // [0, 3]
}

impl LedState {
    pub const OFF: Self = Self { red: 0, green: 0 };
    pub const YELLOW: Self = Self { red: 3, green: 3 };

    fn as_byte(self) -> u8 {
        0b1100 + self.red + (self.green << 4)
    }

    /// Set bipolar knobs to green, unipolar knobs to red.
    pub fn from_knob_value(val: &KnobValue) -> Self {
        match val {
            KnobValue::Bipolar(_) => Self { red: 0, green: 3 },
            KnobValue::Unipolar(_) => Self { red: 3, green: 0 },
        }
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
