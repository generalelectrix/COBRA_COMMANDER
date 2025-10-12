//! Device model for the Akai AMX fader wing.
//!
//! This device actually implements high-precision knobs and faders that send
//! a pair of midi messages on each update, one with the coarse value, and one
//! with the fine value. It would be really nice to take advantage of this.
//! Unfortunately, the current (extremely inside-out) MIDI device model from
//! tunnels assumes that devices are stateless and are passed around with the
//! messages to allow delaying interpretation of the midi messages... we need
//! to fix that original mistake to unwind this.
use log::{error, warn};
use number::UnipolarFloat;
use strum_macros::Display;
use tunnels::{
    clock_bank::ClockIdxExt,
    midi::{cc, event, note_on, Event, EventType, Output},
    midi_controls::{bipolar_from_midi, unipolar_from_midi},
};

use tunnels::clock::{ControlMessage as ClockControlMessage, StateChange as ClockStateChange};
use tunnels::clock_bank::{
    ControlMessage as ClockBankControlMessage, StateChange as ClockBankStateChange,
};

use crate::{
    midi::{Device, MidiHandler},
    show::ShowControlMessage,
    util::{bipolar_fader_with_detent, unipolar_to_range},
};

/// Model of the Akai AMX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AkaiAmx {}

const MIDI_CHANNEL: u8 = 0;

const SEARCH: u8 = 2;
const LOAD: u8 = 4;
const SYNC: u8 = 6;
const CUE: u8 = 8;
const PLAY: u8 = 10;
const HEADPHONE: u8 = 12;

const SEARCH_1: u8 = SEARCH + 1;
const LOAD_1: u8 = LOAD + 1;
const SYNC_1: u8 = SYNC + 1;
const CUE_1: u8 = CUE + 1;
const PLAY_1: u8 = PLAY + 1;
const HEADPHONE_1: u8 = HEADPHONE + 1;

impl AkaiAmx {
    pub const CHANNEL_COUNT: u8 = 2;

    pub fn device_name(&self) -> &str {
        "AMX"
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<AmxControlEvent> {
        use AmxChannelButton::*;
        use AmxChannelControlEvent::*;
        use AmxChannelKnob::*;
        use AmxControlEvent::*;
        // All controls are on channel 1
        if event.mapping.channel != MIDI_CHANNEL {
            return None;
        }
        let control = event.mapping.control;
        Some(match event.mapping.event_type {
            EventType::ControlChange => match control {
                7 => Channel {
                    channel: 0,
                    event: Fader(event.value),
                },
                11 => Channel {
                    channel: 1,
                    event: Fader(event.value),
                },
                10 => knob(0, Treble, event.value),
                9 => knob(0, Mid, event.value),
                8 => knob(0, Bass, event.value),
                15 => knob(0, Filter, event.value),
                55 => knob(0, CueLevel, event.value),
                14 => knob(1, Treble, event.value),
                13 => knob(1, Mid, event.value),
                12 => knob(1, Bass, event.value),
                16 => knob(1, Filter, event.value),
                51 => knob(1, CueLevel, event.value),
                _ => {
                    return None;
                }
            },
            EventType::NoteOn => match control {
                SEARCH => button(0, Search),
                LOAD => button(0, Load),
                SYNC => button(0, Sync),
                CUE => button(0, Cue),
                PLAY => button(0, Play),
                HEADPHONE => button(0, Headphone),
                SEARCH_1 => button(0, Search),
                LOAD_1 => button(0, Load),
                SYNC_1 => button(0, Sync),
                CUE_1 => button(0, Cue),
                PLAY_1 => button(0, Play),
                HEADPHONE_1 => button(0, Headphone),
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
        button: AmxChannelButton,
        state: bool,
        output: &mut Output<Device>,
    ) {
        use AmxChannelButton::*;
        if channel >= Self::CHANNEL_COUNT as usize {
            warn!("AMX channel {channel} out of range for LED state update");
            return;
        }
        let control = channel as u8
            + match button {
                Search => SEARCH,
                Load => LOAD,
                Sync => SYNC,
                Cue => CUE,
                Play => PLAY,
                Headphone => HEADPHONE,
            };
        if let Err(err) = output.send(event(note_on(MIDI_CHANNEL, control), state as u8)) {
            error!("MIDI send error setting LED state {channel}({button}) to {state}: {err}.");
        }
    }

    /// Set one of the VU meters.
    pub fn set_vu_meter(&self, which: VuMeter, value: UnipolarFloat, output: &mut Output<Device>) {
        use VuMeter::*;
        let control = match which {
            Channel1 => 0x40,
            Channel2 => 0x14,
            MasterLeft => 0x3E,
            MasterRight => 0x3F,
        };
        if let Err(err) = output.send(event(
            cc(MIDI_CHANNEL, control),
            unipolar_to_range(0, 85, value),
        )) {
            error!("MIDI send error setting VU meter: {err}");
        }
    }
}

#[allow(dead_code)]
pub enum VuMeter {
    Channel1,
    Channel2,
    MasterLeft,
    MasterRight,
}

#[derive(Clone, Copy, Debug)]
pub enum AmxControlEvent {
    Channel {
        channel: u8,
        event: AmxChannelControlEvent,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum AmxChannelControlEvent {
    Fader(u8),
    Knob { knob: AmxChannelKnob, val: u8 },
    Button(AmxChannelButton),
}

#[derive(Clone, Copy, Debug, Display)]
pub enum AmxChannelButton {
    Search,
    Load,
    Sync,
    Cue,
    Play,
    // Unfortunately, the headphone buttons appear to maintain toggle state on
    // the hardware; they send NoteOn or NoteOff only on each press. If we can
    // control their LEDs we can paper over this...
    Headphone,
}

#[derive(Clone, Copy, Debug, Display)]
pub enum AmxChannelKnob {
    Treble,
    Mid,
    Bass,
    Filter,
    CueLevel,
}

fn knob(channel: u8, knob: AmxChannelKnob, val: u8) -> AmxControlEvent {
    AmxControlEvent::Channel {
        channel,
        event: AmxChannelControlEvent::Knob { knob, val },
    }
}

fn button(channel: u8, button: AmxChannelButton) -> AmxControlEvent {
    AmxControlEvent::Channel {
        channel,
        event: AmxChannelControlEvent::Button(button),
    }
}

impl MidiHandler for AkaiAmx {
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage> {
        use AmxChannelButton::*;
        use AmxChannelControlEvent::*;
        use AmxChannelKnob::*;
        use AmxControlEvent::*;
        Some(ShowControlMessage::Clock(match self.parse(event)? {
            Channel { channel, event } => ClockBankControlMessage {
                channel: ClockIdxExt(channel as usize),
                msg: match event {
                    Fader(val) => ClockControlMessage::Set(ClockStateChange::SubmasterLevel(
                        unipolar_from_midi(val),
                    )),
                    Knob { knob, val } => match knob {
                        Filter => ClockControlMessage::Set(ClockStateChange::Rate(
                            bipolar_fader_with_detent(bipolar_from_midi(val)),
                        )),
                        Bass => ClockControlMessage::Set(ClockStateChange::RateFine(
                            bipolar_fader_with_detent(bipolar_from_midi(val)),
                        )),
                        _ => {
                            return None;
                        }
                    },
                    Button(b) => match b {
                        Sync => ClockControlMessage::Tap,
                        Cue => ClockControlMessage::ToggleOneShot,
                        Play => ClockControlMessage::Retrigger,
                        _ => {
                            return None;
                        }
                    },
                },
            },
        }))
    }

    fn emit_clock_control(&self, msg: &ClockBankStateChange, output: &mut Output<Device>) {
        let channel: usize = msg.channel.into();
        match msg.change {
            ClockStateChange::OneShot(v) => self.set_led(channel, AmxChannelButton::Cue, v, output),
            ClockStateChange::Ticked(v) => self.set_led(channel, AmxChannelButton::Sync, v, output),
            ClockStateChange::Rate(_)
            | ClockStateChange::RateFine(_)
            | ClockStateChange::SubmasterLevel(_)
            | ClockStateChange::UseAudioSize(_)
            | ClockStateChange::UseAudioSpeed(_) => (),
        }
    }

    fn emit_audio_control(&self, msg: &tunnels::audio::StateChange, output: &mut Output<Device>) {
        if let tunnels::audio::StateChange::EnvelopeValue(v) = msg {
            self.set_vu_meter(VuMeter::MasterRight, *v, output);
        }
    }
}
