//! Define midi devices and handle midi controls.

use anyhow::Result;
pub use device::color_organ::ColorOrgan;
use device::{apc20::AkaiApc20, launch_control_xl::NovationLaunchControlXL};
use enum_dispatch::enum_dispatch;
use log::error;
use midi_harness::{
    DeviceChange, DeviceId, DeviceKind, DeviceManager, HandleDeviceChange, InitMidiDevice, Output,
    SlotStatus,
};
use std::{cell::RefCell, fmt::Display, sync::mpsc::Sender};

use crate::{
    animation::StateChange as AnimationStateChange,
    channel::StateChange as ChannelStateChange,
    master::StateChange as MasterStateChange,
    midi::device::{amx::AkaiAmx, cmd_dv1::BehringerCmdDV1, cmd_mm1::BehringerCmdMM1},
    show::ShowControlMessage,
};
use tunnels::{
    midi::{DeviceSpec, Event},
    midi_controls::MidiDevice,
};

use crate::control::ControlMessage;

pub(crate) mod device;
mod mapping;
pub mod slots;

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
}

impl InitMidiDevice for Device {
    fn init_midi(&self, out: &mut dyn Output) -> Result<()> {
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

impl Device {}

/// MIDI handling, interpreting a MIDI event as a channel control message.
#[enum_dispatch]
pub trait MidiHandler {
    /// Interpet an incoming MIDI event as a show control message.
    fn interpret(&self, event: &Event) -> Option<ShowControlMessage>;

    /// Send MIDI state to handle the provided channel state change.
    #[allow(unused_variables)]
    fn emit_channel_control(&self, msg: &ChannelStateChange, output: &mut dyn Output) {}

    /// Send MIDI state to handle the provided clock state change.
    #[allow(unused_variables)]
    fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange, output: &mut dyn Output) {}

    /// Send MIDI state to handle the provided animation state change.
    #[allow(unused_variables)]
    fn emit_animation_control(&self, msg: &AnimationStateChange, output: &mut dyn Output) {}

    /// Send MIDI state to handle the provided audio state change.
    #[allow(unused_variables)]
    fn emit_audio_control(&self, msg: &tunnels::audio::StateChange, output: &mut dyn Output) {}

    /// Send MIDI state to handle the provided master state change.
    #[allow(unused_variables)]
    fn emit_master_control(&self, msg: &MasterStateChange, output: &mut dyn Output) {}
}

pub struct MidiControlMessage {
    pub device: Device,
    pub event: Event,
}

/// Interface MIDI events into a control message channel.
#[derive(Clone)]
pub struct ControlHandler(pub Sender<ControlMessage>);

impl midi_harness::MidiHandler<Device> for ControlHandler {
    fn handle(&self, event: Event, device: &Device) {
        self.0
            .send(ControlMessage::Midi(MidiControlMessage {
                device: *device,
                event,
            }))
            .unwrap();
    }
}

impl HandleDeviceChange for ControlHandler {
    fn on_device_change(&self, change: Result<DeviceChange>) {
        match change {
            Ok(change) => {
                self.0
                    .send(ControlMessage::MidiDeviceChange(change))
                    .unwrap();
            }
            Err(err) => {
                error!(
                    "An error occurred while processing a MIDI device change notification: {err}."
                );
            }
        }
    }
}

/// Immutable-compatible wrapper around the midi manager.
/// Writing to a midi ouput requires a unique reference; we can safely wrap
/// this using RefCell since we only need a reference to the outputs to write,
/// and we can only be making one write call at a time.
pub struct MidiController(RefCell<DeviceManager<Device, ControlHandler>>);

impl MidiController {
    pub fn new(devices: Vec<DeviceSpec<Device>>, send: Sender<ControlMessage>) -> Result<Self> {
        let mut controller = DeviceManager::new(ControlHandler(send));
        for spec in devices {
            controller.add_from_spec(spec.device, spec.input_id, spec.output_id)?;
        }
        Ok(Self(RefCell::new(controller)))
    }

    /// Add a slot without connecting hardware.
    pub fn add_slot(&mut self, name: String, model: Device) -> Result<()> {
        self.0.borrow_mut().add_slot(name, model)
    }

    /// Remove a slot by name.
    pub fn remove_slot(&mut self, name: &str) -> Result<()> {
        self.0.borrow_mut().remove_slot(name)
    }

    /// Connect a MIDI port to a slot.
    pub fn connect_port(
        &mut self,
        slot_name: &str,
        device_id: DeviceId,
        kind: DeviceKind,
    ) -> Result<()> {
        let mut mgr = self.0.borrow_mut();
        match kind {
            DeviceKind::Input => mgr.connect_input(slot_name, device_id),
            DeviceKind::Output => mgr.connect_output(slot_name, device_id),
        }
    }

    /// Clear the device assignment from the named slot.
    pub fn clear_device(&mut self, slot_name: &str) -> Result<()> {
        self.0.borrow_mut().clear_slot(slot_name)
    }

    /// Return the names of all device slots.
    pub fn device_names(&self) -> Vec<String> {
        self.0.borrow().slot_names()
    }

    /// Return a snapshot of the status of every slot.
    pub fn slot_statuses(&self) -> Vec<SlotStatus> {
        self.0.borrow().slot_statuses()
    }

    /// Handle a device appearing or disappearing.
    ///
    /// Return true if we should trigger a UI refresh due to a device reconnecting.
    pub fn handle_device_change(&mut self, change: DeviceChange) -> Result<bool> {
        let Some(reconnected_kind) = self.0.borrow_mut().handle_device_change(change)? else {
            return Ok(false);
        };
        Ok(reconnected_kind == DeviceKind::Output)
    }

    /// Handle a channel state change message.
    pub fn emit_channel_control(&self, msg: &ChannelStateChange) {
        self.0.borrow_mut().visit_outputs(|device, output| {
            device.emit_channel_control(msg, output);
        });
    }

    /// Handle a clock state change message.
    pub fn emit_clock_control(&self, msg: &tunnels::clock_bank::StateChange) {
        self.0.borrow_mut().visit_outputs(|device, output| {
            device.emit_clock_control(msg, output);
        });
    }

    /// Handle a audio state change message.
    pub fn emit_audio_control(&self, msg: &tunnels::audio::StateChange) {
        self.0.borrow_mut().visit_outputs(|device, output| {
            device.emit_audio_control(msg, output);
        });
    }

    /// Handle a animation state change message.
    pub fn emit_animation_control(&self, msg: &AnimationStateChange) {
        self.0.borrow_mut().visit_outputs(|device, output| {
            device.emit_animation_control(msg, output);
        });
    }

    /// Handle a master state change message.
    pub fn emit_master_control(&self, msg: &crate::master::StateChange) {
        self.0.borrow_mut().visit_outputs(|device, output| {
            device.emit_master_control(msg, output);
        });
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
