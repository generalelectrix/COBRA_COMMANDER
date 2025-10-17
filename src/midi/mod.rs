//! Define midi devices and handle midi controls.

use anyhow::Result;
pub use device::color_organ::ColorOrgan;
use device::{apc20::AkaiApc20, launch_control_xl::NovationLaunchControlXL};
use enum_dispatch::enum_dispatch;
use std::{cell::RefCell, fmt::Display, sync::mpsc::Sender};

use crate::{
    animation::StateChange as AnimationStateChange,
    channel::StateChange as ChannelStateChange,
    master::StateChange as MasterStateChange,
    midi::device::{amx::AkaiAmx, cmd_dv1::BehringerCmdDV1, cmd_mm1::BehringerCmdMM1},
    show::ShowControlMessage,
};
use tunnels::{
    midi::{DeviceSpec, Event, Manager, Output},
    midi_controls::MidiDevice,
};

use crate::control::ControlMessage;

mod device;
mod mapping;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[enum_dispatch(MidiHandler)]
pub enum Device {
    #[allow(unused)]
    Apc20(AkaiApc20),
    LaunchControlXL(NovationLaunchControlXL),
    CmdMM1(BehringerCmdMM1),
    Amx(AkaiAmx),
    CmdDV1(BehringerCmdDV1),
    ColorOrgan(ColorOrgan),
}

impl Display for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.device_name())
    }
}

impl MidiDevice for Device {
    fn device_name(&self) -> &str {
        match self {
            Device::Apc20(d) => d.device_name(),
            Device::LaunchControlXL(d) => d.device_name(),
            Device::CmdMM1(d) => d.device_name(),
            Device::Amx(d) => d.device_name(),
            Device::CmdDV1(d) => d.device_name(),
            Device::ColorOrgan(d) => d.device_name(),
        }
    }

    fn init_midi(&self, out: &mut Output) -> Result<()> {
        match self {
            Device::Apc20(d) => d.init_midi(out),
            Device::LaunchControlXL(d) => d.init_midi(out),
            Device::CmdMM1(d) => d.init_midi(out),
            Device::Amx(d) => d.init_midi(out),
            Device::CmdDV1(d) => d.init_midi(out),
            Device::ColorOrgan(d) => d.init_midi(out),
        }
    }
}

impl Device {
    /// Return all known MIDI device types that match the parameters.
    pub fn all(internal_clocks: bool) -> Vec<Self> {
        let mut devices = vec![
            // Self::Apc20(AkaiApc20 { channel_offset: 0 }),
            Self::LaunchControlXL(NovationLaunchControlXL { channel_offset: 0 }),
            Self::LaunchControlXL(NovationLaunchControlXL { channel_offset: 8 }),
            Self::CmdDV1(BehringerCmdDV1 {}),
        ];
        if internal_clocks {
            devices.push(Self::CmdMM1(BehringerCmdMM1 {}));
            devices.push(Self::Amx(AkaiAmx {}));
        }
        devices
    }

    /// Attempt to identify connected devices to automatically configure MIDI.
    pub fn auto_configure(
        internal_clocks: bool,
        input_ports: &[String],
        output_ports: &[String],
    ) -> Vec<DeviceSpec<Self>> {
        // For all known devices, see if we have a matching input and output port.
        Self::all(internal_clocks)
            .into_iter()
            .filter_map(|device| {
                let name = device.device_name().to_string();
                (input_ports.contains(&name) && output_ports.contains(&name)).then_some(
                    DeviceSpec {
                        device,
                        input_port_name: name.clone(),
                        output_port_name: name,
                    },
                )
            })
            .collect()
    }
}

/// MIDI handling, interpreting a MIDI event as a channel control message.
#[enum_dispatch]
pub trait MidiHandler {
    /// Interpet an incoming MIDI event as a show control message.
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage>;

    /// Send MIDI state to handle the provided channel state change.
    #[allow(unused_variables)]
    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut Output) {}

    /// Send MIDI state to handle the provided clock state change.
    #[allow(unused_variables)]
    fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange, output: &mut Output) {}

    /// Send MIDI state to handle the provided animation state change.
    #[allow(unused_variables)]
    fn emit_animation_control(&self, msg: &AnimationStateChange, output: &mut Output) {}

    /// Send MIDI state to handle the provided audio state change.
    #[allow(unused_variables)]
    fn emit_audio_control(&self, msg: &tunnels::audio::StateChange, output: &mut Output) {}

    /// Send MIDI state to handle the provided master state change.
    #[allow(unused_variables)]
    fn emit_master_control(&self, msg: &MasterStateChange, output: &mut Output) {}
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
        for (device, output) in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are stateless
            device.emit_channel_control(msg, output);
        }
    }

    /// Handle a clock state change message.
    pub fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange) {
        for (device, output) in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are stateless
            device.emit_clock_control(msg, output);
        }
    }

    /// Handle a audio state change message.
    pub fn emit_audio_control(&self, msg: &tunnels::audio::StateChange) {
        for (device, output) in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are stateless
            device.emit_audio_control(msg, output);
        }
    }

    /// Handle a animation state change message.
    pub fn emit_animation_control(&self, msg: &AnimationStateChange) {
        for (device, output) in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are stateless
            device.emit_animation_control(msg, output);
        }
    }

    /// Handle a master state change message.
    pub fn emit_master_control(&self, msg: &crate::master::StateChange) {
        for (device, output) in self.0.borrow_mut().outputs() {
            // FIXME: tunnels devices are stateless
            device.emit_master_control(msg, output);
        }
    }
}

pub trait EmitMidiChannelMessage {
    fn emit_midi_channel_message(&self, msg: &ChannelStateChange);
}

pub trait EmitMidiMasterMessage {
    fn emit_midi_master_message(&self, msg: &crate::master::StateChange);
}

pub trait EmitMidiAnimationMessage {
    fn emit_midi_animation_message(&self, msg: &crate::animation::StateChange);
}
