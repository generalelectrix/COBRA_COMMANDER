//! Top-level traits and types for control events.

use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};

use anyhow::{Result, bail};
use rosc::OscMessage;
use tunnels::midi::{CreateControlEvent, DeviceSpec};

use crate::{
    midi::{
        Device, EmitMidiAnimationMessage, EmitMidiChannelMessage, EmitMidiMasterMessage,
        MidiControlMessage, MidiController,
    },
    osc::{
        EmitOscMessage, EmitScopedOscMessage, OscClientId, OscControlMessage, OscControlResponse,
        OscController, ScopedControlEmitter, TalkbackMode,
    },
};

/// Emit scoped control messages.
/// Will be extended in the future to potentially cover more cases.
pub trait EmitScopedControlMessage: EmitScopedOscMessage + EmitMidiAnimationMessage {}

impl<T> EmitScopedControlMessage for T where T: EmitScopedOscMessage + EmitMidiAnimationMessage {}

/// Emit control messages.
/// Will be extended in the future to potentially cover more cases.
pub trait EmitControlMessage:
    EmitOscMessage + EmitMidiChannelMessage + EmitMidiMasterMessage + EmitMidiAnimationMessage
{
}

impl<T> EmitControlMessage for T where
    T: EmitOscMessage + EmitMidiChannelMessage + EmitMidiMasterMessage + EmitMidiAnimationMessage
{
}

/// Handle receiving and responding to show control messages.
pub struct Controller {
    osc: OscController,
    midi: MidiController,
    recv: Receiver<ControlMessage>,
}

impl Controller {
    pub fn new(
        receive_port: u16,
        osc_controllers: Vec<OscClientId>,
        midi_devices: Vec<DeviceSpec<Device>>,
    ) -> Result<Self> {
        let (send, recv) = channel();
        Ok(Self {
            osc: OscController::new(receive_port, osc_controllers, send.clone())?,
            midi: MidiController::new(midi_devices, send)?,
            recv,
        })
    }

    pub fn recv(&self, timeout: Duration) -> Result<Option<ControlMessage>> {
        match self.recv.recv_timeout(timeout) {
            Ok(msg) => Ok(Some(msg)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                bail!("control receiver disconnected");
            }
        }
    }

    /// Register a new OSC client.
    pub fn register_osc_client(&mut self, client_id: OscClientId) {
        self.osc.register(client_id);
    }

    /// Deregister an OSC client.
    pub fn deregister_osc_client(&mut self, client_id: OscClientId) {
        self.osc.deregister(client_id);
    }

    /// Return a decorated version of self that will include the provided
    /// metadata when sending OSC response messages.
    pub fn sender_with_metadata<'a>(
        &'a mut self,
        sender_id: Option<&'a OscClientId>,
    ) -> ControlMessageWithMetadataSender<'a> {
        ControlMessageWithMetadataSender {
            sender_id,
            controller: self,
        }
    }
}

impl tunnels::audio::EmitStateChange for Controller {
    fn emit_audio_state_change(&mut self, sc: tunnels::audio::StateChange) {
        crate::osc::audio::emit_osc_state_change(
            &sc,
            &ScopedControlEmitter {
                entity: crate::osc::audio::GROUP,
                emitter: &ControlMessageWithMetadataSender {
                    sender_id: None,
                    controller: self,
                },
            },
        );
        self.midi.emit_audio_control(&sc);
    }
}

impl tunnels::clock_bank::EmitStateChange for Controller {
    fn emit_clock_bank_state_change(&mut self, sc: tunnels::clock_bank::StateChange) {
        crate::osc::clock::emit_osc_state_change(
            &sc,
            &ScopedControlEmitter {
                entity: crate::osc::clock::GROUP,
                emitter: &ControlMessageWithMetadataSender {
                    sender_id: None,
                    controller: self,
                },
            },
        );
        self.midi.emit_clock_control(&sc);
    }
}

/// Decorate the Controller to add message metedata to control responses.
pub struct ControlMessageWithMetadataSender<'a> {
    pub sender_id: Option<&'a OscClientId>,
    pub controller: &'a mut Controller,
}

impl<'a> EmitOscMessage for ControlMessageWithMetadataSender<'a> {
    fn emit_osc(&self, msg: OscMessage) {
        self.controller.osc.send(OscControlResponse {
            sender_id: self.sender_id.cloned(),
            talkback: TalkbackMode::All, // FIXME: hardcoded talkback
            msg,
        });
    }
}

impl<'a> EmitMidiChannelMessage for ControlMessageWithMetadataSender<'a> {
    fn emit_midi_channel_message(&self, msg: &crate::channel::StateChange) {
        self.controller.midi.emit_channel_control(msg);
    }
}

impl<'a> EmitMidiAnimationMessage for ControlMessageWithMetadataSender<'a> {
    fn emit_midi_animation_message(&self, msg: &crate::animation::StateChange) {
        self.controller.midi.emit_animation_control(msg);
    }
}

impl<'a> EmitMidiMasterMessage for ControlMessageWithMetadataSender<'a> {
    fn emit_midi_master_message(&self, msg: &crate::master::StateChange) {
        self.controller.midi.emit_master_control(msg);
    }
}

pub enum ControlMessage {
    RegisterClient(OscClientId),
    DeregisterClient(OscClientId),
    Osc(OscControlMessage),
    Midi(MidiControlMessage),
}

impl CreateControlEvent<Device> for ControlMessage {
    fn from_event(event: tunnels::midi::Event, device: Device) -> Self {
        Self::Midi(MidiControlMessage { device, event })
    }
}

pub mod mock {
    use super::*;

    /// An emitter that does nothing.
    ///
    /// Useful for tests, as well as occasional use as a shim when creating
    /// composite fixture types.
    pub struct NoOpEmitter;

    impl EmitOscMessage for NoOpEmitter {
        fn emit_osc(&self, _: OscMessage) {}
    }

    impl EmitMidiChannelMessage for NoOpEmitter {
        fn emit_midi_channel_message(&self, _: &crate::channel::StateChange) {}
    }

    impl EmitMidiAnimationMessage for NoOpEmitter {
        fn emit_midi_animation_message(&self, _: &crate::animation::StateChange) {}
    }

    impl EmitMidiMasterMessage for NoOpEmitter {
        fn emit_midi_master_message(&self, _: &crate::master::StateChange) {}
    }

    impl EmitScopedOscMessage for NoOpEmitter {
        fn emit_float(&self, _: &str, _: f64) {}
        fn emit_osc(&self, _: crate::osc::ScopedOscMessage) {}
    }
}
