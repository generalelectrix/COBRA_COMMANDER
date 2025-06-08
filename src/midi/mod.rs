//! Define midi devices and handle midi controls.

use anyhow::Result;
use device::{apc20::AkaiApc20, cmd_mm1::BehringerCmdMM1, launch_control_xl::NovationLaunchControlXL};
use std::{cell::RefCell, fmt::Display, sync::mpsc::Sender};

use crate::{channel::StateChange as ChannelStateChange, show::ShowControlMessage};
use tunnels::{
    midi::{DeviceSpec, Event, Manager, Output},
    midi_controls::MidiDevice,
};

use crate::control::ControlMessage;

mod device;
mod mapping;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    #[allow(unused)]
    Apc20(AkaiApc20),
    CmdMM1(BehringerCmdMM1),
    LaunchControlXL(NovationLaunchControlXL),
    LaunchControlXLSecondWing(NovationLaunchControlXL),
}

impl Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.device_name())
    }
}

impl MidiDevice for Device {
    fn device_name(&self) -> &str {
        match self {
            Self::Apc20(d) => d.device_name(),
            Self::CmdMM1(d) => d.device_name(),
            Self::LaunchControlXL(d) => d.device_name(),
            Self::LaunchControlXLSecondWing(_) => "Launch Control XL Second Wing",
        }
    }

    fn init_midi(&self, out: &mut tunnels::midi::Output<Self>) -> Result<()> {
        match self {
            Self::Apc20(d) => d.init_midi(out),
            Self::CmdMM1(d) => d.init_midi(out),
            Self::LaunchControlXL(d) => d.init_midi(out),
            Self::LaunchControlXLSecondWing(d) => d.init_midi(out),
        }
    }
}

impl Device {
    /// Return all known midi device types.
    pub fn all() -> Vec<Self> {
        vec![
            // Self::Apc20(AkaiApc20 { channel_offset: 0 }),
            Self::CmdMM1(BehringerCmdMM1),
            Self::LaunchControlXL(NovationLaunchControlXL { channel_offset: 0 }),
            Self::LaunchControlXLSecondWing(NovationLaunchControlXL { channel_offset: 8 }),
        ]
    }
}

impl MidiHandler for Device {
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage> {
        match self {
            Self::Apc20(d) => d.interpret(event),
            Self::CmdMM1(d) => d.interpret(event),
            Self::LaunchControlXL(d) => d.interpret(event),
            Self::LaunchControlXLSecondWing(d) => d.interpret(event),
        }
    }

    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut Output<Device>) {
        match self {
            Self::Apc20(d) => d.emit_channel_control(msg, output),
            Self::CmdMM1(d) => d.emit_channel_control(msg, output),
            Self::LaunchControlXL(d) => d.emit_channel_control(msg, output),
            Self::LaunchControlXLSecondWing(d) => d.emit_channel_control(msg, output),
        }
    }

    fn emit_master_control(&self, msg: &crate::master::StateChange, output: &mut Output<Device>) {
        match self {
            Self::Apc20(d) => d.emit_master_control(msg, output),
            Self::CmdMM1(d) => d.emit_master_control(msg, output),
            Self::LaunchControlXL(d) => d.emit_master_control(msg, output),
            Self::LaunchControlXLSecondWing(d) => d.emit_master_control(msg, output),
        }
    }

    fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange, output: &mut Output<Device>) {
        match self {
            Self::Apc20(_) => {},
            Self::CmdMM1(d) => d.emit_clock_control(msg, output),
            Self::LaunchControlXL(_) => {},
            Self::LaunchControlXLSecondWing(_) => {},
        }
    }
}

/// MIDI handling, interpreting a MIDI event as a channel control message.
pub trait MidiHandler {
    /// Interpet an incoming MIDI event as a show control message.
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage>;

    /// Send MIDI state to handle the provided channel state change.
    #[allow(unused)]
    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut Output<Device>) {}

    /// Send MIDI state to handle the provided master state change.
    #[allow(unused)]
    fn emit_master_control(&self, msg: &crate::master::StateChange, output: &mut Output<Device>) {}

    /// Send MIDI state to handle the provided clock state change.
    #[allow(unused)]
    fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange, output: &mut Output<Device>) {}
}

pub struct MidiControlMessage {
    pub device: Device,
    pub event: Event,
}

/// Immutable-compatible wrapper around the midi manager.
/// Writing to a midi ouput requires a unique reference; we can safely wrap
/// this using RefCell since we only need a reference to the outputs to write,
/// and we can only be making one write call at a time.
pub struct MidiController(RefCell<Manager<Device>>);

impl MidiController {
    pub fn new(devices: Vec<DeviceSpec<Device>>, send: Sender<ControlMessage>) -> Result<Self> {
        let mut controller = Manager::default();
        for d in devices {
            controller.add_device(d, send.clone())?;
        }
        Ok(Self(RefCell::new(controller)))
    }

    /// Handle a channel state change message.
    pub fn emit_channel_control(&self, msg: &ChannelStateChange) {
        for output in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are inside-out/stateless
            let device = *output.device();
            device.emit_channel_control(msg, output);
        }
    }

    /// Handle a master state change message.
    pub fn emit_master_control(&self, msg: &crate::master::StateChange) {
        for output in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are inside-out/stateless
            let device = *output.device();
            device.emit_master_control(msg, output);
        }
    }

    /// Handle a clock state change message.
    pub fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange) {
        for output in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are inside-out/stateless
            let device = *output.device();
            device.emit_clock_control(msg, output);
        }
    }
}

impl EmitMidiChannelMessage for MidiController {
    fn emit_midi_channel_message(&self, msg: &ChannelStateChange) {
        self.emit_channel_control(msg);
    }
}

impl EmitMidiMasterMessage for MidiController {
    fn emit_midi_master_message(&self, msg: &crate::master::StateChange) {
        self.emit_master_control(msg);
    }
}

impl EmitMidiClockMessage for MidiController {
    fn emit_midi_clock_message(&self, msg: &tunnels::clock_bank::StateChange) {
        self.emit_clock_control(msg);
    }
}

pub trait EmitMidiChannelMessage {
    fn emit_midi_channel_message(&self, msg: &ChannelStateChange);
}

pub trait EmitMidiMasterMessage {
    fn emit_midi_master_message(&self, msg: &crate::master::StateChange);
}

pub trait EmitMidiClockMessage {
    fn emit_midi_clock_message(&self, msg: &tunnels::clock_bank::StateChange);
}
