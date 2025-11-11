//! Device model for the Behringer CMD MM-1 fader wing.
use log::{debug, error};
use number::UnipolarFloat;
use strum_macros::Display;
use tunnels::{
    clock_bank::ClockIdxExt,
    midi::{Event, EventType, Output, cc, event, note_on},
    midi_controls::{
        MidiDevice,
        audio::{envelope_edge_from_midi, filter_from_midi, gain_from_midi},
        bipolar_from_midi, unipolar_from_midi,
    },
};

use tunnels::audio::StateChange as AudioStateChange;
use tunnels::clock::{ControlMessage as ClockControlMessage, StateChange as ClockStateChange};
use tunnels::clock_bank::{
    ControlMessage as ClockBankControlMessage, StateChange as ClockBankStateChange,
};

use crate::{midi::MidiHandler, show::ShowControlMessage, util::unipolar_to_range};

/// Model of the Behringer CMD MM-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehringerCmdMM1 {}

const MIDI_CHANNEL: u8 = 4;

impl MidiDevice for BehringerCmdMM1 {
    fn device_name(&self) -> &str {
        "CMD MM-1"
    }
}

impl BehringerCmdMM1 {
    pub const CHANNEL_COUNT: u8 = 4;

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
        Some(match event.mapping.event_type {
            EventType::ControlChange => {
                match control {
                    1 => SmallKnob {
                        index: 0,
                        val: event.value,
                    },
                    2 => SmallKnob {
                        index: 1,
                        val: event.value,
                    },
                    4 => SmallKnob {
                        index: 2,
                        val: event.value,
                    },
                    5 => SmallKnob {
                        index: 3,
                        val: event.value,
                    },
                    6..=21 => {
                        // Figure out which row and channel.
                        let index = control - 6;
                        let row = index / 4;
                        let channel = index % 4;
                        Channel {
                            channel,
                            event: Knob {
                                row,
                                val: event.value,
                            },
                        }
                    }
                    48..=51 => {
                        let channel = control - 48;
                        Channel {
                            channel,
                            event: Fader(event.value),
                        }
                    }
                    // TODO: handle other knobs
                    _ => {
                        return None;
                    }
                }
            }
            EventType::NoteOn => match control {
                48..=51 => Channel {
                    channel: control - 48,
                    event: Button(Cue),
                },
                19 | 23 | 27 | 31 => Channel {
                    channel: (control - 19) / 4,
                    event: Button(One),
                },
                20 | 24 | 28 | 32 => Channel {
                    channel: (control - 20) / 4,
                    event: Button(Two),
                },
                16 => Single(CmdMM1Single::Left),
                17 => Single(CmdMM1Single::Right),
                18 => Single(CmdMM1Single::Monitor),
                _ => {
                    return None;
                }
            },
            _ => {
                return None;
            }
        })
    }

    /// Set a button LED on or off.
    pub fn set_led(
        &self,
        channel: usize,
        button: CmdMM1ChannelButton,
        state: bool,
        output: &mut Output,
    ) {
        if channel >= Self::CHANNEL_COUNT as usize {
            debug!("CMD MM-1 channel {channel} out of range for LED state update");
            return;
        }
        let control = match button {
            CmdMM1ChannelButton::Cue => 48 + channel as u8,
            CmdMM1ChannelButton::One => 19 + (channel as u8 * 4),
            CmdMM1ChannelButton::Two => 20 + (channel as u8 * 4),
        };
        if let Err(err) = output.send(event(note_on(MIDI_CHANNEL, control), state as u8)) {
            error!("MIDI send error setting LED state {channel}({button}) to {state}: {err}.");
        }
    }

    /// Set one of the VU meters.
    ///
    /// which: pass false for left, true for right
    pub fn set_vu_meter(&self, which: bool, value: UnipolarFloat, output: &mut Output) {
        let control = if which { 81 } else { 80 };
        // Why they chose to scale the VU meters from 48 to 63... shrug.
        let scaled_val = unipolar_to_range(48, 63, value);
        if let Err(err) = output.send(event(cc(MIDI_CHANNEL, control), scaled_val)) {
            error!("MIDI send error setting VU meter LED state: {err}.");
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CmdMM1ControlEvent {
    Channel {
        channel: u8,
        event: CmdMM1ChannelControlEvent,
    },
    /// The small knobs at the top of the device.
    /// Indexed left to right.
    SmallKnob {
        index: u8,
        val: u8,
    },
    Single(CmdMM1Single),
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
    /// "1" button
    One,
    /// "2" button
    Two,
    /// "CUE" button
    Cue,
}

#[derive(Clone, Copy, Debug)]
pub enum CmdMM1Single {
    /// "Left" button
    Left,
    /// "Right" button
    Right,
    /// Small square button below VU meters.
    Monitor,
}

impl MidiHandler for BehringerCmdMM1 {
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage> {
        use CmdMM1ChannelButton::*;
        use CmdMM1ChannelControlEvent::*;
        use CmdMM1ControlEvent::*;
        Some(match self.parse(event)? {
            Channel { channel, event } => ShowControlMessage::Clock(ClockBankControlMessage {
                channel: ClockIdxExt(channel as usize),
                msg: match event {
                    Fader(val) => ClockControlMessage::Set(ClockStateChange::SubmasterLevel(
                        unipolar_from_midi(val),
                    )),
                    Knob { row, val } => match row {
                        0 => {
                            ClockControlMessage::Set(ClockStateChange::Rate(bipolar_from_midi(val)))
                        }
                        3 => ClockControlMessage::Set(ClockStateChange::RateFine(
                            bipolar_from_midi(val),
                        )),
                        _ => {
                            return None;
                        }
                    },
                    Button(b) => match b {
                        Cue => ClockControlMessage::Tap,
                        One => ClockControlMessage::ToggleOneShot,
                        Two => ClockControlMessage::Retrigger,
                    },
                },
            }),
            SmallKnob { index, val } => {
                use tunnels::audio::ControlMessage::Set;
                use tunnels::audio::StateChange::*;
                match index {
                    0 => {
                        ShowControlMessage::Audio(Set(EnvelopeAttack(envelope_edge_from_midi(val))))
                    }
                    1 => ShowControlMessage::Audio(Set(EnvelopeRelease(envelope_edge_from_midi(
                        val,
                    )))),
                    2 => ShowControlMessage::Audio(Set(FilterCutoff(filter_from_midi(val)))),
                    3 => ShowControlMessage::Audio(Set(Gain(gain_from_midi(val)))),
                    _ => {
                        return None;
                    }
                }
            }
            Single(event) => match event {
                CmdMM1Single::Monitor => {
                    ShowControlMessage::Audio(tunnels::audio::ControlMessage::ToggleMonitor)
                }
                _ => {
                    return None;
                }
            },
        })
    }

    fn emit_clock_control(&self, msg: &ClockBankStateChange, output: &mut Output) {
        let channel: usize = msg.channel.into();
        match msg.change {
            ClockStateChange::OneShot(v) => {
                self.set_led(channel, CmdMM1ChannelButton::One, v, output)
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

    fn emit_audio_control(&self, msg: &AudioStateChange, output: &mut Output) {
        if let Err(err) = match msg {
            AudioStateChange::Monitor(v) => output.send(event(note_on(MIDI_CHANNEL, 18), *v as u8)),
            AudioStateChange::EnvelopeValue(v) => {
                self.set_vu_meter(true, *v, output);
                Ok(())
            }
            AudioStateChange::IsClipping(v) => {
                self.set_vu_meter(
                    false,
                    if *v {
                        UnipolarFloat::ONE
                    } else {
                        UnipolarFloat::ZERO
                    },
                    output,
                );
                Ok(())
            }
            _ => Ok(()),
        } {
            error!("MIDI error updating audio control for {msg:?}: {err}.");
        }
    }
}
