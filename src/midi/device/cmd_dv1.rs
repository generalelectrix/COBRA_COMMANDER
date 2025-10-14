//! Device model for the Behringer CMD DV-1 encoder/button wing.
//!
//! This device had so much potential but is hobbled by maintaining too much
//! on-board state. It reserves 13 of the buttons for on-board modal changes
//! that change the MIDI controls sent by the different banks of controls, and
//! there is no way to change this behavior.
//!
//! As such, we paper of this by putting a piece of tape over all of the buttons
//! that act as modal selectors; then, we map all 3 or 4 possible controls to the
//! same binding, and send LED or encoder ring state to all 3 or 4 possible
//! controls on every update.  It still has a lot of buttons, but it sure would
//! have been nice to get those other 13 back...

use log::{debug, error};
use number::{BipolarFloat, UnipolarFloat};
use strum_macros::Display;
use tunnels::{
    clock_bank::ClockIdxExt,
    midi::{cc, event, note_on, Event, EventType, Output},
    midi_controls::{bipolar_from_midi, unipolar_from_midi},
};

use tunnels::audio::StateChange as AudioStateChange;
use tunnels::clock::{ControlMessage as ClockControlMessage, StateChange as ClockStateChange};
use tunnels::clock_bank::{
    ControlMessage as ClockBankControlMessage, StateChange as ClockBankStateChange,
};

use crate::{
    midi::{Device, MidiHandler},
    show::ShowControlMessage,
    util::unipolar_to_range,
};

/// Model of the Behringer CMD DV-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BehringerCmdDV1 {}

const MIDI_CHANNEL: u8 = 6;

impl BehringerCmdDV1 {
    pub fn device_name(&self) -> &str {
        "CMD DV-1"
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<CmdDV1ControlEvent> {
        use CmdDV1ControlEvent::*;
        if event.mapping.channel != MIDI_CHANNEL {
            return None;
        }
        let control = event.mapping.control;
        let v = event.value;
        match event.mapping.event_type {
            EventType::ControlChange => match control {
                // Top row
                20 | 24 | 28 | 32 => encoder(0, v),
                21 | 25 | 29 | 33 => encoder(1, v),
                22 | 26 | 30 | 34 => encoder(2, v),
                23 | 27 | 31 | 35 => encoder(3, v),
                // Middle row
                36 | 40 | 44 | 48 => encoder(4, v),
                37 | 41 | 45 | 49 => encoder(5, v),
                38 | 42 | 46 | 50 => encoder(6, v),
                39 | 43 | 47 | 51 => encoder(7, v),
                // Bottom row
                64..68 => encoder(control - 56, v),
                _ => None,
            },
            EventType::NoteOn => Some(match control {
                // Top row
                20 | 24 | 28 | 32 => Button(0),
                21 | 25 | 29 | 33 => Button(1),
                22 | 26 | 30 | 34 => Button(2),
                23 | 27 | 31 | 35 => Button(3),
                // Middle row
                36 | 40 | 44 | 48 => Button(4),
                37 | 41 | 45 | 49 => Button(5),
                38 | 42 | 46 | 50 => Button(6),
                39 | 43 | 47 | 51 => Button(7),
                // Third row of modal buttons
                64 | 68 | 72 => Button(8),
                65 | 69 | 73 => Button(9),
                66 | 70 | 74 => Button(10),
                67 | 71 | 75 => Button(11),
                80..96 => Button(control - 68),
                _ => {
                    return None;
                }
            }),
            _ => None,
        }
    }

    /// Set a button LED on or off.
    pub fn set_led(&self, index: u8, state: bool, output: &mut Output<Device>) {
        if index >= 28 {
            error!("CMD DV-1 button {index} out of range for LED state update");
            return;
        }
        // How many extra commands do we need to send to cover our bases?
        let (start, duplicates) = match index {
            0..4 => (20 + index, 4),
            4..8 => (36 + (index - 4), 4),
            8..12 => (64 + (index - 8), 3),
            _ => (index + 68, 1),
        };
        for i in 0..duplicates {
            let control = start + (4 * i);
            if let Err(err) = output.send(event(note_on(MIDI_CHANNEL, control), state as u8)) {
                error!("MIDI send error setting LED state {index} to {state}: {err}.");
            }
        }
    }

    /// Set an encoder ring.
    ///
    /// Note that the encoder rings have 15 LEDs, and acccept a value between 1 and 16.
    /// Need to debug this once the device is here. No checking is done that value
    /// is in range.
    pub fn set_encoder_raw(&self, index: u8, value: u8, output: &mut Output<Device>) {
        if index >= 12 {
            error!("CMD DV-1 encoder {index} out of range for LED state update");
            return;
        }
        // How many extra commands do we need to send to cover our bases?
        let (start, duplicates) = match index {
            0..4 => (20 + index, 4),
            4..8 => (36 + (index - 4), 4),
            _ => (index + 56, 1),
        };
        for i in 0..duplicates {
            let control = start + (4 * i);
            if let Err(err) = output.send(event(cc(MIDI_CHANNEL, control), value)) {
                error!("MIDI send error setting VU meter LED state: {err}.");
            }
        }
    }

    /// Set an encoder ring from a unipolar value.
    pub fn set_encoder_unipolar(
        &self,
        index: u8,
        value: UnipolarFloat,
        output: &mut Output<Device>,
    ) {
        // FIXME: this range is probably wrong and likely includes the setting that
        // doesn't show a position.
        self.set_encoder_raw(index, unipolar_to_range(1, 16, value), output);
    }

    /// Set an encoder ring from a bipolar value.
    pub fn set_encoder_bipolar(&self, index: u8, value: BipolarFloat, output: &mut Output<Device>) {
        // FIXME: this range is probably wrong; confirm that 0 is centered
        self.set_encoder_unipolar(index, value.rescale_as_unipolar(), output);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncoderStep {
    Left,
    Right,
}

impl EncoderStep {
    /// Convert a MIDI value into an encoder step.
    /// The encoders send 63 for a left turn, 65 for a right turn.
    ///
    /// Return None for any other value.
    fn from_val(v: u8) -> Option<EncoderStep> {
        match v {
            63 => Some(EncoderStep::Left),
            65 => Some(EncoderStep::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CmdDV1ControlEvent {
    Encoder {
        /// Top to bottom, left to right. Top-left is 0, bottom-right is 11.
        index: u8,
        step: EncoderStep,
    },
    /// Top to bottom, left to right.
    ///
    /// Every row has 4 buttons. 7 rows of non-modal-shift buttons.
    /// The first three rows have modal shift behavior. The last four do not.
    Button(u8),
}

fn encoder(index: u8, v: u8) -> Option<CmdDV1ControlEvent> {
    let step = EncoderStep::from_val(v)?;
    Some(CmdDV1ControlEvent::Encoder { index, step })
}
