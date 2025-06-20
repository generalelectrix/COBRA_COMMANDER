//! A generic midi device for representing a MIDI keyboard.
use anyhow::{ensure, Result};
use number::{Phase, UnipolarFloat};

use color_organ::{ControlMessage, ReleaseId};
use tunnels::{
    midi::{Event, EventType},
    midi_controls::unipolar_from_midi,
};

use crate::{
    fixture::color::{HsluvRenderer, HSLUV_LIGHTNESS_OFFSET},
    midi::MidiHandler,
    show::ShowControlMessage,
};

/// Abtract over a MIDI keyboard used to drive a color organ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorOrgan {
    // TODO: since hue is a phase, we can use this as a width and correctly handle
    // all of the notes outside of this range; then the octave shift will move the
    // spectrum, but it won't just stop working entirely.
    /// MIDI note for the lowest key on the keyboard.
    /// Beware of octave shift keys.
    note_low: u8,
    /// MIDI note for the highest key on the keyboard.
    /// Beware of octabe shift keys.
    note_high: u8,
    /// MIDI channel to listen for color events on.
    channel: u8,
}

impl ColorOrgan {
    fn new(note_low: u8, note_high: u8, channel: u8) -> Result<Self> {
        ensure!(
            note_low > note_high,
            "invalid note range: {note_high} is not larger than {note_low}"
        );
        Ok(Self {
            note_low,
            note_high,
            channel,
        })
    }
}

impl MidiHandler for ColorOrgan {
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage> {
        if event.mapping.channel != self.channel {
            return None;
        }
        if event.mapping.event_type == EventType::ControlChange {
            return None;
        }
        if !(self.note_low..=self.note_high).contains(&event.mapping.control) {
            return None;
        }

        let note = event.mapping.control;
        Some(ShowControlMessage::ColorOrgan(
            if event.mapping.event_type == EventType::NoteOff {
                ControlMessage::NoteOff(note as ReleaseId)
            } else {
                let hue = Phase::new(
                    (event.mapping.control - self.note_low) as f64
                        / (self.note_high - self.note_low) as f64,
                );
                // FIXME: need to push saturation and lightness control down
                ControlMessage::NoteOn {
                    color: HsluvRenderer {
                        hue,
                        sat: UnipolarFloat::ONE,
                        lightness: HSLUV_LIGHTNESS_OFFSET,
                    },
                    velocity: unipolar_from_midi(event.value),
                    release_id: event.mapping.control as ReleaseId,
                }
            },
        ))
    }
}
