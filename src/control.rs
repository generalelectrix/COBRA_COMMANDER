//! Top-level traits and types for control events.

use std::{
    fmt,
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use midi_harness::DeviceChange;
use rosc::OscMessage;
use tunnels::midi::DeviceSpec;

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
        send: Sender<ControlMessage>,
        recv: Receiver<ControlMessage>,
    ) -> Result<Self> {
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

    /// Add a MIDI device.
    pub fn add_midi_device(&mut self, spec: DeviceSpec<Device>) -> Result<()> {
        self.midi.add_device(spec)
    }

    /// Clear the device assignment from a MIDI slot.
    pub fn clear_midi_device(&mut self, slot_name: &str) -> Result<()> {
        self.midi.clear_device(slot_name)
    }

    /// Handle a MIDI device change.
    pub fn handle_device_change(&mut self, change: DeviceChange) -> Result<bool> {
        self.midi.handle_device_change(change)
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

#[cfg(test)]
impl Controller {
    pub fn test_new() -> (Self, Sender<ControlMessage>) {
        let (send, recv) = std::sync::mpsc::channel();
        let (osc, _osc_recv) = OscController::test_new();
        let controller = Self {
            osc,
            midi: MidiController::new(vec![], send.clone()).unwrap(),
            recv,
        };
        (controller, send)
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

/// The result of processing a MetaCommand.
pub type CommandResponse = std::result::Result<(), String>;

/// A handle for sending commands to the show and waiting for responses.
///
/// Cloneable — any thread can hold one.
#[derive(Clone)]
pub struct CommandClient {
    send: Sender<ControlMessage>,
    zmq_ctx: zmq::Context,
}

impl CommandClient {
    pub fn new(send: Sender<ControlMessage>, zmq_ctx: zmq::Context) -> Self {
        Self { send, zmq_ctx }
    }

    pub fn zmq_ctx(&self) -> &zmq::Context {
        &self.zmq_ctx
    }

    /// Send a command and block until the show responds.
    pub fn send_command(&self, cmd: MetaCommand) -> Result<CommandResponse> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.send
            .send(ControlMessage::Meta(cmd, Some(reply_tx)))
            .map_err(|_| anyhow::anyhow!("show control channel disconnected"))?;
        reply_rx.recv().context("show did not send a response")
    }
}

/// Commands for show-level meta-control: configuration changes,
/// system actions, and lifecycle events.
///
/// Any source with a Sender<ControlMessage> can send these.
pub enum MetaCommand {
    ReloadPatch,
    RefreshUI,
    ResetAllAnimations,
    StartAnimationVisualizer,
    AssignDmxPort {
        universe: usize,
        port: Box<dyn rust_dmx::DmxPort>,
    },
    AddMidiDevice(DeviceSpec<Device>),
    #[expect(unused)]
    ClearMidiDevice {
        slot_name: String,
    },
    UseClockService(crate::clock_service::ClockService),
    SetAudioDevice(String),
}

impl fmt::Debug for MetaCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReloadPatch => write!(f, "ReloadPatch"),
            Self::RefreshUI => write!(f, "RefreshUI"),
            Self::ResetAllAnimations => write!(f, "ResetAllAnimations"),
            Self::StartAnimationVisualizer => write!(f, "StartAnimationVisualizer"),
            Self::AssignDmxPort { universe, port } => f
                .debug_struct("AssignDmxPort")
                .field("universe", universe)
                .field("port", &format_args!("{port}"))
                .finish(),
            Self::AddMidiDevice(spec) => f
                .debug_struct("AddMidiDevice")
                .field("device", &spec.device)
                .finish(),
            Self::ClearMidiDevice { slot_name } => write!(f, "ClearMidiDevice({slot_name})"),
            Self::UseClockService(_) => write!(f, "UseClockService"),
            Self::SetAudioDevice(name) => write!(f, "SetAudioDevice({name})"),
        }
    }
}

/// Translate an OSC control message (already known to be in the "Meta" group)
/// into a MetaCommand.
///
/// Returns `Ok(None)` when the message is valid but should be ignored.
pub fn meta_command_from_osc(msg: &OscControlMessage) -> Result<Option<MetaCommand>> {
    match msg.control() {
        "ReloadPatch" => Ok(Some(MetaCommand::ReloadPatch)),
        "RefreshUI" => {
            if msg.get_bool()? {
                Ok(Some(MetaCommand::RefreshUI))
            } else {
                Ok(None)
            }
        }
        "ResetAllAnimations" => Ok(Some(MetaCommand::ResetAllAnimations)),
        unknown => bail!("unknown Meta control {}", unknown),
    }
}

pub enum ControlMessage {
    RegisterClient(OscClientId),
    DeregisterClient(OscClientId),
    MidiDeviceChange(DeviceChange),
    Osc(OscControlMessage),
    Midi(MidiControlMessage),
    /// A meta-command with an optional reply channel for the response.
    Meta(MetaCommand, Option<Sender<CommandResponse>>),
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{OscClientId, OscControlMessage};
    use rosc::{OscMessage, OscType};

    /// Helper: construct an OscControlMessage with the given address and arg.
    fn make_meta_osc(control: &str, arg: OscType) -> OscControlMessage {
        OscControlMessage::new(
            OscMessage {
                addr: format!("/Meta/{control}"),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    #[test]
    fn meta_command_from_osc_reload_patch() {
        let msg = make_meta_osc("ReloadPatch", OscType::Float(1.0));
        let cmd = meta_command_from_osc(&msg).unwrap().unwrap();
        assert!(matches!(cmd, MetaCommand::ReloadPatch));
    }

    #[test]
    fn meta_command_from_osc_refresh_ui_truthy() {
        let msg = make_meta_osc("RefreshUI", OscType::Float(1.0));
        let cmd = meta_command_from_osc(&msg).unwrap().unwrap();
        assert!(matches!(cmd, MetaCommand::RefreshUI));
    }

    #[test]
    fn meta_command_from_osc_refresh_ui_falsy_is_none() {
        let msg = make_meta_osc("RefreshUI", OscType::Float(0.0));
        let result = meta_command_from_osc(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn meta_command_from_osc_reset_all_animations() {
        let msg = make_meta_osc("ResetAllAnimations", OscType::Float(1.0));
        let cmd = meta_command_from_osc(&msg).unwrap().unwrap();
        assert!(matches!(cmd, MetaCommand::ResetAllAnimations));
    }

    #[test]
    fn meta_command_from_osc_unknown_is_err() {
        let msg = make_meta_osc("DoSomethingWeird", OscType::Float(1.0));
        let result = meta_command_from_osc(&msg);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DoSomethingWeird"));
    }

    #[test]
    fn meta_command_debug_formats_port_display() {
        use crate::dmx::mock::MockDmxPort;
        let cmd = MetaCommand::AssignDmxPort {
            universe: 3,
            port: Box::new(MockDmxPort::new()),
        };
        let debug = format!("{cmd:?}");
        assert!(debug.contains("AssignDmxPort"));
        assert!(debug.contains("universe: 3"));
        assert!(debug.contains("mock"));
    }
}
