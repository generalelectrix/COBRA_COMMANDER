use log::{debug, error};
use tunnels::midi::{Event, EventType, Mapping, Output};


/// Basic model for the few APC20 controls we use so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AkaiApc20 {
    /// When interpreting channel control messages, offset the incoming channel
    /// by this amount.
    pub channel_offset: usize,
}

const FADER: u8 = 0x7;
const TRACK_SELECT: u8 = 0x33;

impl AkaiApc20 {
    pub const CHANNEL_COUNT: u8 = 8;

    pub fn device_name(&self) -> &str {
        "Akai APC20"
    }

    /// Put into ableton (full control) mode.
    pub fn init_midi(&self, out: &mut Output) -> anyhow::Result<()> {
        debug!("Sending APC20 sysex mode command.");
        out.send_raw(&[
            0xF0, 0x47, 0x7F, 0x7B, 0x60, 0x00, 0x04, 0x42, 0x08, 0x02, 0x01, 0xF7,
        ])?;
        Ok(())
    }

    /// Interpret a midi event as a typed control event.
    pub fn parse(&self, event: &Event) -> Option<Apc20ControlEvent> {
        use Apc20ChannelButtonType::*;
        use Apc20ChannelControlEvent::*;
        use Apc20ControlEvent::*;
        match event.mapping.event_type {
            EventType::ControlChange => match event.mapping.control {
                FADER => Some(Channel {
                    channel: event.mapping.channel,
                    event: Fader(event.value),
                }),
                _ => None,
            },
            EventType::NoteOn => match event.mapping.control {
                TRACK_SELECT => Some(Channel {
                    channel: event.mapping.channel,
                    event: Button(TrackSelect),
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// Process a state change and emit midi.
    pub fn emit(&self, sc: Apc20StateChange, output: &mut Output) {
        use Apc20ChannelButtonType::*;
        use Apc20StateChange::*;
        match sc {
            ChannelButtonRadio { channel, button } => {
                let control = match button {
                    TrackSelect => TRACK_SELECT,
                };
                for c in 0..8 {
                    if let Err(err) = output.send(Event {
                        mapping: Mapping {
                            event_type: EventType::NoteOn,
                            channel: c,
                            control,
                        },
                        value: if Some(c) == channel { 127 } else { 0 },
                    }) {
                        error!("midi send error for APC20: {err}");
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub enum Apc20ControlEvent {
    Channel {
        channel: u8,
        event: Apc20ChannelControlEvent,
    },
    // SpecialButton(Apc20SpecialButtonType),
}

#[derive(Clone, Copy)]
pub enum Apc20ChannelControlEvent {
    Fader(u8),
    Button(Apc20ChannelButtonType),
}

#[derive(Clone, Copy)]
pub enum Apc20ChannelButtonType {
    // Record,
    // Solo,
    // Activator,
    TrackSelect,
    // ClipStop,
    // ClipLaunch(u8), // payload is the row, 0 is top row
}

#[allow(unused)]
#[derive(Clone, Copy)]
pub enum Apc20SpecialButtonType {
    SceneLaunch(u8), // payload is the row, 0 is top row
    Shift,
    MasterSelect,
}

#[derive(Clone, Copy)]
pub enum Apc20StateChange {
    // SingleChannelButton {
    //     channel: u8,
    //     button: Apc20ChannelButtonType,
    //     on: bool, // TODO: model blinking
    // },
    /// Set the specified channel on, all others off
    /// If channel is None, turn all buttons off.
    ChannelButtonRadio {
        channel: Option<u8>,
        button: Apc20ChannelButtonType,
    },
}
