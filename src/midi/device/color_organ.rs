//! A generic midi device for representing a MIDI keyboard.
use log::{debug, error};
use tunnels::{
    midi::{Event, EventType, Mapping, Output},
    midi_controls::MidiDevice,
};

use crate::midi::MidiHandler;

/// Abtract over a MIDI keyboard used to drive a color organ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorOrgan {
    /// MIDI note for the lowest key on the keyboard.
    /// Beware of octave shift keys.
    pub note_low: u8,
    /// MIDI note for the highest key on the keyboard.
    /// Beware of octabe shift keys.
    pub note_high: u8,
    /// MIDI channel to listen for color events on.
    pub channel: u8,
}

impl MidiHandler for ColorOrgan {
    fn interpret(&self, event: &Event) -> Option<crate::show::ShowControlMessage> {
        None
    }
}
