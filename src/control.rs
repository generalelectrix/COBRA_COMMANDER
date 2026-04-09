//! Top-level traits and types for control events.

use std::{
    fmt,
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use midi_harness::{DeviceChange, SlotStatus};
use rosc::OscMessage;
use tunnels::midi::DeviceSpec;

use crate::{
    midi::{
        Device, EmitMidiAnimationMessage, EmitMidiChannelMessage, EmitMidiMasterMessage,
        MidiControlMessage, MidiController,
    },
    osc::{
        EmitOscMessage, EmitScopedOscMessage, OscClientId, OscClientListener, OscControlMessage,
        OscControlResponse, OscController, ScopedControlEmitter, TalkbackMode,
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

    /// Connect a MIDI port to a slot.
    pub fn connect_midi_port(
        &mut self,
        slot_name: &str,
        device_id: midi_harness::DeviceId,
        kind: midi_harness::DeviceKind,
    ) -> Result<()> {
        self.midi.connect_port(slot_name, device_id, kind)
    }

    /// Deregister an OSC client.
    pub fn deregister_osc_client(&mut self, client_id: OscClientId) {
        self.osc.deregister(client_id);
    }

    /// Get a listener handle for the shared OSC client list.
    pub fn osc_client_listener(&self) -> OscClientListener {
        self.osc.client_listener()
    }

    /// Clear the device assignment from a MIDI slot.
    pub fn clear_midi_device(&mut self, slot_name: &str) -> Result<()> {
        self.midi.clear_device(slot_name)
    }

    /// Add a MIDI slot without connecting hardware.
    pub fn add_midi_slot(&mut self, name: String, model: Device) -> Result<()> {
        self.midi.add_slot(name, model)
    }

    /// Remove a MIDI slot by name.
    pub fn remove_midi_slot(&mut self, name: &str) -> Result<()> {
        self.midi.remove_slot(name)
    }

    /// Return the names of all MIDI device slots.
    pub fn midi_slot_names(&self) -> Vec<String> {
        self.midi.device_names()
    }

    /// Return a snapshot of the status of every MIDI device slot.
    pub fn midi_slot_statuses(&self) -> Vec<SlotStatus> {
        self.midi.slot_statuses()
    }

    /// Ensure the correct number of submaster wing slots exist for the
    /// given channel count.
    pub fn reconcile_submaster_wings(&mut self, channel_count: usize) -> Result<()> {
        use crate::midi::{
            device::launch_control_xl::NovationLaunchControlXL,
            slots::{is_submaster_wing, submaster_wing_count, submaster_wing_name},
        };

        let desired = submaster_wing_count(channel_count);
        let slot_names = self.midi_slot_names();
        let current = slot_names.iter().filter(|n| is_submaster_wing(n)).count();

        for i in (current + 1)..=desired {
            let model = Device::LaunchControlXL(NovationLaunchControlXL {
                channel_offset: (i - 1) * 8,
            });
            self.add_midi_slot(submaster_wing_name(i), model)?;
        }
        for i in ((desired + 1)..=current).rev() {
            self.remove_midi_slot(&submaster_wing_name(i))?;
        }
        Ok(())
    }

    /// Ensure the clock wing slot exists iff `needs_clock_wing` is true.
    pub fn reconcile_clock_wing(&mut self, needs_clock_wing: bool) -> Result<()> {
        use crate::midi::{device::cmd_mm1::BehringerCmdMM1, slots::CLOCK_WING_SLOT};

        let has = self.midi_slot_names().iter().any(|n| n == CLOCK_WING_SLOT);

        if needs_clock_wing && !has {
            self.add_midi_slot(
                CLOCK_WING_SLOT.to_string(),
                Device::CmdMM1(BehringerCmdMM1 {}),
            )?;
        } else if !needs_clock_wing && has {
            self.remove_midi_slot(CLOCK_WING_SLOT)?;
        }
        Ok(())
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
}

impl CommandClient {
    pub fn new(send: Sender<ControlMessage>) -> Self {
        Self { send }
    }

    /// Send a command and block until the show responds.
    pub fn send_command(&self, cmd: MetaCommand) -> Result<()> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.send
            .send(ControlMessage::Meta(cmd, Some(reply_tx)))
            .map_err(|_| anyhow::anyhow!("show control channel disconnected"))?;
        reply_rx
            .recv()
            .context("show did not send a response")?
            .map_err(|e| anyhow::anyhow!(e))
    }
}

/// Commands for show-level meta-control: configuration changes,
/// system actions, and lifecycle events.
///
/// Any source with a Sender<ControlMessage> can send these.
pub enum MetaCommand {
    RefreshUI,
    ResetAllAnimations,
    AssignDmxPort {
        universe: usize,
        port: Box<dyn rust_dmx::DmxPort>,
    },
    ClearMidiDevice {
        slot_name: String,
    },
    ConnectMidiPort {
        slot_name: String,
        device_id: midi_harness::DeviceId,
        kind: midi_harness::DeviceKind,
    },
    UseClockService(crate::clock_service::ClockService),
    UseInternalClocks(Option<String>),
    RegisterOscClient(OscClientId),
    DropOscClient(OscClientId),
    /// Apply a new patch configuration from the GUI editor.
    Repatch(Vec<crate::config::FixtureGroupConfig>),
    /// Enable or disable the master strobe fader channel.
    SetMasterStrobeChannel(bool),
}

impl fmt::Debug for MetaCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshUI => write!(f, "RefreshUI"),
            Self::ResetAllAnimations => write!(f, "ResetAllAnimations"),
            Self::AssignDmxPort { universe, port } => f
                .debug_struct("AssignDmxPort")
                .field("universe", universe)
                .field("port", &format_args!("{port}"))
                .finish(),
            Self::ClearMidiDevice { slot_name } => write!(f, "ClearMidiDevice({slot_name})"),
            Self::ConnectMidiPort {
                slot_name, kind, ..
            } => write!(f, "ConnectMidiPort({slot_name}, {kind:?})"),
            Self::UseClockService(_) => write!(f, "UseClockService"),
            Self::UseInternalClocks(device) => write!(f, "UseInternalClocks({device:?})"),
            Self::RegisterOscClient(id) => write!(f, "RegisterOscClient({id})"),
            Self::DropOscClient(id) => write!(f, "DropOscClient({id})"),
            Self::Repatch(groups) => write!(f, "Repatch({} groups)", groups.len()),
            Self::SetMasterStrobeChannel(enable) => {
                write!(f, "SetMasterStrobeChannel({enable})")
            }
        }
    }
}

/// Translate an OSC control message (already known to be in the "Meta" group)
/// into a MetaCommand.
///
/// Returns `Ok(None)` when the message is valid but should be ignored.
pub fn meta_command_from_osc(msg: &OscControlMessage) -> Result<Option<MetaCommand>> {
    match msg.control() {
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
    MidiDeviceChange(DeviceChange),
    Osc(OscControlMessage),
    Midi(MidiControlMessage),
    /// A meta-command with an optional reply channel for the response.
    Meta(MetaCommand, Option<Sender<CommandResponse>>),
}

#[cfg(test)]
pub mod mock {
    use super::*;

    /// Create a CommandClient that auto-responds Ok(()) to every command.
    pub fn auto_respond_client() -> CommandClient {
        let (send, recv) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(ControlMessage::Meta(_, Some(reply))) = recv.recv() {
                let _ = reply.send(Ok(()));
            }
        });
        CommandClient::new(send)
    }

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

    use crate::midi::slots;

    fn submaster_wing_count(controller: &Controller) -> usize {
        controller
            .midi_slot_names()
            .iter()
            .filter(|n| slots::is_submaster_wing(n))
            .count()
    }

    fn has_clock_wing(controller: &Controller) -> bool {
        controller
            .midi_slot_names()
            .iter()
            .any(|n| n == slots::CLOCK_WING_SLOT)
    }

    #[test]
    fn reconcile_submaster_wings_one_channel() {
        let (mut controller, _send) = Controller::test_new();
        controller.reconcile_submaster_wings(1).unwrap();
        assert_eq!(submaster_wing_count(&controller), 1);
        assert_eq!(controller.midi_slot_names()[0], "Submaster Wing 1");
    }

    #[test]
    fn reconcile_submaster_wings_grows() {
        let (mut controller, _send) = Controller::test_new();
        controller.reconcile_submaster_wings(1).unwrap();
        assert_eq!(submaster_wing_count(&controller), 1);

        // 9 channels → 2 wings
        controller.reconcile_submaster_wings(9).unwrap();
        assert_eq!(submaster_wing_count(&controller), 2);
    }

    #[test]
    fn reconcile_submaster_wings_shrinks() {
        let (mut controller, _send) = Controller::test_new();
        controller.reconcile_submaster_wings(9).unwrap();
        assert_eq!(submaster_wing_count(&controller), 2);

        controller.reconcile_submaster_wings(1).unwrap();
        assert_eq!(submaster_wing_count(&controller), 1);
    }

    #[test]
    fn reconcile_submaster_wings_zero_channels_still_one() {
        let (mut controller, _send) = Controller::test_new();
        controller.reconcile_submaster_wings(0).unwrap();
        assert_eq!(submaster_wing_count(&controller), 1);
    }

    #[test]
    fn reconcile_clock_wing_adds_when_needed() {
        let (mut controller, _send) = Controller::test_new();
        assert!(!has_clock_wing(&controller));

        controller.reconcile_clock_wing(true).unwrap();
        assert!(has_clock_wing(&controller));
    }

    #[test]
    fn reconcile_clock_wing_removes_when_not_needed() {
        let (mut controller, _send) = Controller::test_new();
        controller.reconcile_clock_wing(true).unwrap();
        assert!(has_clock_wing(&controller));

        controller.reconcile_clock_wing(false).unwrap();
        assert!(!has_clock_wing(&controller));
    }

    #[test]
    fn reconcile_clock_wing_noop_when_already_correct() {
        let (mut controller, _send) = Controller::test_new();

        // No clock wing, don't need one — no-op.
        controller.reconcile_clock_wing(false).unwrap();
        assert!(!has_clock_wing(&controller));

        // Has clock wing, still need one — no-op.
        controller.reconcile_clock_wing(true).unwrap();
        controller.reconcile_clock_wing(true).unwrap();
        assert!(has_clock_wing(&controller));
    }
}
