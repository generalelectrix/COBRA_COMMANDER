//! Device model for the Behringer CMD MM-1.
use log::debug;
use tunnels::{
    clock_bank::{ClockIdxExt, ControlMessage as ClockBankControlMessage},
    clock::{ControlMessage as ClockControlMessage, StateChange as ClockChange},
    midi::{cc, event, note_on, Event, EventType, Mapping, Output},
    midi_controls::{bipolar_from_midi, bipolar_to_midi, unipolar_from_midi, unipolar_to_midi, MidiDevice},
};

use crate::{midi::Device, show::ShowControlMessage};

/// Model of the Behringer CMD MM-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehringerCmdMM1;

const MIDI_CHANNEL: u8 = 4;

// Control mappings
const RATE_CC_BASE: u8 = 6;
const LEVEL_CC_BASE: u8 = 48;
const TAP_NOTE_BASE: u8 = 48;
const ONESHOT_NOTE_BASE: u8 = 19;
const RETRIGGER_NOTE_BASE: u8 = 20;
const NOTE_SPACING: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Control {
    Rate,
    Level,
    Tap,
    OneShot,
    Retrigger,
}

impl BehringerCmdMM1 {
    pub const CHANNEL_COUNT: usize = 4;

    pub fn device_name(&self) -> &str {
        "Behringer CMD MM-1"
    }

    /// No special initialization required for CMD MM-1.
    pub fn init_midi<D: MidiDevice>(&self, _out: &mut Output<D>) -> anyhow::Result<()> {
        debug!("CMD MM-1 initialized (no special initialization required).");
        Ok(())
    }

    /// Get the mapping for a specific control and channel.
    fn mapping(&self, control: Control, channel: usize) -> Option<Mapping> {
        if channel >= Self::CHANNEL_COUNT {
            return None;
        }

        let channel = channel as u8;

        match control {
            Control::Rate => Some(cc(MIDI_CHANNEL, RATE_CC_BASE + channel)),
            Control::Level => Some(cc(MIDI_CHANNEL, LEVEL_CC_BASE + channel)),
            Control::Tap => Some(note_on(MIDI_CHANNEL, TAP_NOTE_BASE + channel)),
            Control::OneShot => Some(note_on(MIDI_CHANNEL, ONESHOT_NOTE_BASE + channel * NOTE_SPACING)),
            Control::Retrigger => Some(note_on(MIDI_CHANNEL, RETRIGGER_NOTE_BASE + channel * NOTE_SPACING)),
        }
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<CmdMM1ControlEvent> {
        if event.mapping.channel != MIDI_CHANNEL {
            return None;
        }

        match event.mapping.event_type {
            EventType::ControlChange => {
                // Check for Rate controls
                if (RATE_CC_BASE..RATE_CC_BASE + Self::CHANNEL_COUNT as u8).contains(&event.mapping.control) {
                    let channel = event.mapping.control - RATE_CC_BASE;
                    return Some(CmdMM1ControlEvent::Rate { channel, value: event.value });
                }
                // Check for Level controls
                if (LEVEL_CC_BASE..LEVEL_CC_BASE + Self::CHANNEL_COUNT as u8).contains(&event.mapping.control) {
                    let channel = event.mapping.control - LEVEL_CC_BASE;
                    return Some(CmdMM1ControlEvent::Level { channel, value: event.value });
                }
            }
            EventType::NoteOn => {
                // Check for Tap controls
                if (TAP_NOTE_BASE..TAP_NOTE_BASE + Self::CHANNEL_COUNT as u8).contains(&event.mapping.control) {
                    let channel = event.mapping.control - TAP_NOTE_BASE;
                    return Some(CmdMM1ControlEvent::Tap { channel });
                }
                // Check for OneShot controls (with spacing)
                for ch in 0..Self::CHANNEL_COUNT as u8 {
                    if event.mapping.control == ONESHOT_NOTE_BASE + ch * NOTE_SPACING {
                        return Some(CmdMM1ControlEvent::OneShot { channel: ch });
                    }
                    if event.mapping.control == RETRIGGER_NOTE_BASE + ch * NOTE_SPACING {
                        return Some(CmdMM1ControlEvent::Retrigger { channel: ch });
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// Process a clock state change and emit midi for LED feedback.
    pub fn emit(&self, channel: usize, sc: &ClockChange, output: &mut Output<Device>) {
        use ClockChange::*;

        let mut send = |control: Control, value: u8| {
            if let Some(mapping) = self.mapping(control, channel) {
                if let Err(err) = output.send(event(mapping, value)) {
                    log::error!("midi send error for CMD MM-1: {err}");
                }
            }
        };

        match sc {
            Retrigger(v) => send(Control::Retrigger, *v as u8),
            OneShot(v) => send(Control::OneShot, *v as u8),
            Ticked(v) => send(Control::Tap, *v as u8),
            Rate(v) => send(Control::Rate, bipolar_to_midi(*v)),
            SubmasterLevel(v) => send(Control::Level, unipolar_to_midi(*v)),
            UseAudioSize(v) => send(Control::OneShot, *v as u8), // Map to OneShot LED since no dedicated control
            UseAudioSpeed(v) => send(Control::Retrigger, *v as u8), // Map to Retrigger LED since no dedicated control
        }
    }
}

#[derive(Clone, Copy)]
pub enum CmdMM1ControlEvent {
    Rate { channel: u8, value: u8 },
    Level { channel: u8, value: u8 },
    Tap { channel: u8 },
    OneShot { channel: u8 },
    Retrigger { channel: u8 },
}

impl CmdMM1ControlEvent {
    /// Convert this control event into a show control message.
    pub fn to_show_control_message(self) -> ShowControlMessage {
        use ClockChange::*;
        use ClockControlMessage::*;

        let (channel, msg) = match self {
            CmdMM1ControlEvent::Rate { channel, value } => (
                channel,
                Set(Rate(bipolar_from_midi(value))),
            ),
            CmdMM1ControlEvent::Level { channel, value } => (
                channel,
                Set(SubmasterLevel(unipolar_from_midi(value))),
            ),
            CmdMM1ControlEvent::Tap { channel } => (channel, Tap),
            CmdMM1ControlEvent::OneShot { channel } => (channel, ToggleOneShot),
            CmdMM1ControlEvent::Retrigger { channel } => (channel, ToggleRetrigger),
        };

        ShowControlMessage::Clock(ClockBankControlMessage {
            channel: ClockIdxExt(channel as usize),
            msg,
        })
    }
}