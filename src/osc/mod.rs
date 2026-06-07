use crate::channel::{ChannelBinding, ChannelStateChange, ChannelStateEmitter};
use crate::config::GroupName;
use crate::control::ControlMessage;
use crate::control::EmitControlMessage;
use crate::midi::{EmitMidiAnimationMessage, EmitMidiMasterMessage};
use crate::osc::listener::{OscListener, PendingSocket};
use crate::osc::sender::OscSender;
use anyhow::Result;
use anyhow::{Context, bail};
use log::warn;
use number::{BipolarFloat, Phase, UnipolarFloat};
use rosc::{OscMessage, OscType};
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::Display;
use std::net::{SocketAddr, UdpSocket};
#[cfg(test)]
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;

use self::radio_button::RadioButton;

pub mod animation;
pub mod audio;
mod basic_controls;
mod bipolar_array;
mod button_array;
pub mod channels;
pub mod clock;
mod control_message;
mod label_array;
mod listener;
pub mod positioner;
mod radio_button;
mod sender;
mod unipolar_array;

pub use control_message::OscControlMessage;

/// Emit an implicitly-scoped OSC message.
pub trait EmitScopedOscMessage {
    fn emit_osc(&self, msg: ScopedOscMessage);

    /// Send an OSC message setting the state of a float control.
    fn emit_float(&self, control: &str, val: f64) {
        self.emit_osc(ScopedOscMessage {
            control,
            arg: OscType::Float(val as f32),
        });
    }
}

pub trait EmitOscMessage {
    fn emit_osc(&self, msg: OscMessage);
}

pub struct OscController {
    send: Sender<OscControlResponse>,
    client_manager: sender::OscClientManager,
    /// Handoff slot the listener polls to adopt a rebound receive socket.
    pending_socket: PendingSocket,
}

/// The OSC receive port tried at startup.
pub const DEFAULT_RECEIVE_PORT: u16 = 8000;

/// An OSC receive socket and the port it listens on. Moved across threads to
/// hand a socket bound on one thread to the listener running on another.
#[derive(Debug)]
pub struct BoundOsc {
    pub socket: UdpSocket,
    pub port: u16,
}

impl BoundOsc {
    /// Bind OSC input on `port` across all interfaces.
    pub fn bind(port: u16) -> Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", port))
            .with_context(|| format!("failed to bind OSC receive port {port}"))?;
        Ok(Self { socket, port })
    }
}

impl OscController {
    pub fn new(
        socket: UdpSocket,
        send_addrs: Vec<OscClientId>,
        send: Sender<ControlMessage>,
    ) -> Result<Self> {
        let (client_manager, initial_listener) = sender::OscClientManager::new(send_addrs);

        let pending_socket: PendingSocket = Arc::new(Mutex::new(None));
        let mut listener = OscListener::from_socket(
            client_manager.listener(),
            socket,
            pending_socket.clone(),
            send,
        );

        thread::spawn(move || {
            listener.run();
        });

        let (mut sender, response_send) =
            OscSender::new(initial_listener).context("failed to start OSC sender")?;

        thread::spawn(move || {
            sender.run();
        });

        Ok(Self {
            send: response_send,
            client_manager,
            pending_socket,
        })
    }

    /// Give the OSC listener a bound socket to receive on, replacing the one it
    /// holds.
    pub fn swap_socket(&self, socket: UdpSocket) {
        if let Ok(mut slot) = self.pending_socket.lock() {
            *slot = Some(socket);
        }
    }

    /// Send an OSC message to all clients.
    pub fn send(&self, msg: OscControlResponse) {
        if self.send.send(msg).is_err() {
            warn!("OSC send channel is disconnected.");
        }
    }

    /// Register an OSC client.
    pub fn register(&self, client_id: OscClientId) {
        self.client_manager.register(client_id);
    }

    /// Deregister an OSC client.
    pub fn deregister(&self, client_id: OscClientId) {
        self.client_manager.deregister(client_id);
    }

    /// Snapshot the current OSC client list.
    pub fn client_ids(&self) -> Vec<OscClientId> {
        self.client_manager.client_ids()
    }
}

#[cfg(test)]
impl OscController {
    pub fn test_new() -> (Self, std::sync::mpsc::Receiver<OscControlResponse>) {
        let (send, recv) = std::sync::mpsc::channel();
        let (client_manager, _) = sender::OscClientManager::new(vec![]);
        (
            Self {
                send,
                client_manager,
                pending_socket: Arc::new(Mutex::new(None)),
            },
            recv,
        )
    }
}

/// Decorate a control message emitter to inject a group name into the address.
pub struct FixtureStateEmitter<'a> {
    name: &'a GroupName,
    channel_emitter: ChannelStateEmitter<'a>,
}

impl<'a> FixtureStateEmitter<'a> {
    pub fn new(name: &'a GroupName, channel_emitter: ChannelStateEmitter<'a>) -> Self {
        Self {
            name,
            channel_emitter,
        }
    }

    pub fn emit_channel(&self, msg: ChannelStateChange) {
        self.channel_emitter.emit(msg);
    }

    /// The channel binding of the addressed group: whether it is the
    /// currently-selected channel, another channel, or unbound.
    pub fn channel(&self) -> &ChannelBinding {
        self.channel_emitter.channel()
    }

    /// Build a sibling [`ScopedControlEmitter`] with a different entity
    /// scope, reusing the same underlying control message sender.
    pub fn scoped(&self, entity: &'a str) -> ScopedControlEmitter<'a> {
        ScopedControlEmitter {
            entity,
            emitter: self.channel_emitter.inner(),
        }
    }
}

impl<'a> EmitScopedOscMessage for FixtureStateEmitter<'a> {
    fn emit_osc(&self, msg: ScopedOscMessage) {
        let addr = format!("/{}/{}", self.name, msg.control);
        self.channel_emitter.emit_osc(OscMessage {
            addr,
            args: vec![msg.arg],
        });
    }
}

pub struct ScopedControlEmitter<'a> {
    pub entity: &'a str,
    pub emitter: &'a dyn EmitControlMessage,
}

impl<'a> EmitScopedOscMessage for ScopedControlEmitter<'a> {
    fn emit_osc(&self, msg: ScopedOscMessage) {
        self.emitter.emit_osc(OscMessage {
            addr: format!("/{}/{}", self.entity, msg.control),
            args: vec![msg.arg],
        });
    }
}

impl<'a> EmitMidiAnimationMessage for ScopedControlEmitter<'a> {
    fn emit_midi_animation_message(&self, msg: &crate::animation::StateChange) {
        self.emitter.emit_midi_animation_message(msg);
    }
}

impl<'a> EmitMidiMasterMessage for ScopedControlEmitter<'a> {
    fn emit_midi_master_message(&self, msg: &crate::master::StateChange) {
        self.emitter.emit_midi_master_message(msg);
    }
}

/// An OSC message that is implicitly scoped to a particular entity.
/// Only the name of the control and the value to be sent are required.
/// TODO: decide how to handle situations where we need more address.
pub struct ScopedOscMessage<'a> {
    pub control: &'a str,
    pub arg: OscType,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Deserialize)]
pub struct OscClientId(SocketAddr);

impl OscClientId {
    pub fn addr(&self) -> &SocketAddr {
        &self.0
    }

    #[cfg(test)]
    pub fn example() -> Self {
        Self(SocketAddr::from_str("127.0.0.1:9999").unwrap())
    }

    #[cfg(test)]
    pub fn from_addr(addr: SocketAddr) -> Self {
        Self(addr)
    }
}

impl Display for OscClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OSC client at {}", self.0)
    }
}

type ControlMessageCreator<C> =
    Box<dyn Fn(&OscControlMessage) -> Result<Option<(C, TalkbackMode)>>>;

pub type Control = String;

pub struct GroupControlMap<C>(HashMap<Control, ControlMessageCreator<C>>);

impl<C> Default for GroupControlMap<C> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<C> core::fmt::Debug for GroupControlMap<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} control mappings>", self.0.len())
    }
}

impl<C> GroupControlMap<C> {
    pub fn handle(&self, msg: &OscControlMessage) -> Result<Option<(C, TalkbackMode)>> {
        let control = msg.control();
        let Some(handler) = self.0.get(control) else {
            bail!("no control handler matched \"{control}\"");
        };
        handler(msg)
    }

    pub fn add<F>(&mut self, control: &str, handler: F)
    where
        F: Fn(&OscControlMessage) -> Result<Option<C>> + 'static,
    {
        match self.0.entry(control.to_string()) {
            Entry::Occupied(_) => {
                panic!("duplicate control definition \"{control}\"");
            }
            Entry::Vacant(v) => v.insert(Box::new(move |m| {
                Ok(handler(m)?.map(|msg| (msg, TalkbackMode::All)))
            })),
        };
    }

    pub fn add_fetch_process<F, T, P>(&mut self, control: &str, fetch: F, process: P)
    where
        F: Fn(&OscControlMessage) -> Result<T, OscError> + 'static,
        P: Fn(T) -> Option<C> + 'static,
    {
        self.add(control, move |v| Ok(process(fetch(v)?)))
    }

    pub fn add_unipolar<F>(&mut self, control: &str, process: F)
    where
        F: Fn(UnipolarFloat) -> C + 'static,
    {
        self.add_fetch_process(control, OscControlMessage::get_unipolar, move |v| {
            Some(process(v))
        })
    }

    pub fn add_bipolar<F>(&mut self, control: &str, process: F)
    where
        F: Fn(BipolarFloat) -> C + 'static,
    {
        self.add_fetch_process(control, OscControlMessage::get_bipolar, move |v| {
            Some(process(v))
        })
    }

    #[expect(unused)]
    pub fn add_bool<F>(&mut self, control: &str, process: F)
    where
        F: Fn(bool) -> C + 'static,
    {
        self.add_fetch_process(control, OscControlMessage::get_bool, move |v| {
            Some(process(v))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TalkbackMode {
    /// Control responses should be sent to all clients.
    All,
    /// Control responses should be sent to all clients except the sender.
    Off,
}

pub struct OscControlResponse {
    pub sender_id: Option<OscClientId>,
    pub talkback: TalkbackMode,
    pub msg: OscMessage,
}

#[derive(Debug, Error)]
#[error("{addr}: {msg}")]
pub struct OscError {
    pub addr: String,
    pub msg: String,
}

impl OscControlMessage {
    /// Get a single float argument from the provided OSC message.
    fn get_float(&self) -> Result<f64, OscError> {
        match &self.arg {
            OscType::Float(v) => Ok(*v as f64),
            OscType::Double(v) => Ok(*v),
            other => Err(self.err(format!(
                "expected a single float argument but found {other:?}"
            ))),
        }
    }

    /// Get a single unipolar float argument from the provided OSC message.
    pub fn get_unipolar(&self) -> Result<UnipolarFloat, OscError> {
        Ok(UnipolarFloat::new(self.get_float()?))
    }

    /// Get a single bipolar float argument from the provided OSC message.
    pub fn get_bipolar(&self) -> Result<BipolarFloat, OscError> {
        Ok(BipolarFloat::new(self.get_float()?))
    }

    /// Get a single phase argument from the provided OSC message.
    pub fn get_phase(&self) -> Result<Phase, OscError> {
        Ok(Phase::new(self.get_float()?))
    }

    /// Get a single boolean argument from the provided OSC message.
    /// Coerce ints and floats to boolean values.
    pub fn get_bool(&self) -> Result<bool, OscError> {
        let bval = match &self.arg {
            OscType::Bool(b) => *b,
            OscType::Int(i) => *i != 0,
            OscType::Float(v) => *v != 0.0,
            OscType::Double(v) => *v != 0.0,
            other => {
                return Err(self.err(format!(
                    "expected a single bool argument but found {other:?}"
                )));
            }
        };
        Ok(bval)
    }
}

#[cfg(test)]
pub struct MockEmitter {
    pub messages: std::cell::RefCell<Vec<(String, OscType)>>,
}

#[cfg(test)]
impl MockEmitter {
    pub fn new() -> Self {
        Self {
            messages: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn take(&self) -> Vec<(String, OscType)> {
        self.messages.borrow_mut().drain(..).collect()
    }
}

#[cfg(test)]
impl EmitScopedOscMessage for MockEmitter {
    fn emit_osc(&self, msg: ScopedOscMessage) {
        self.messages
            .borrow_mut()
            .push((msg.control.to_string(), msg.arg));
    }
}

pub mod prelude {
    pub use super::FixtureStateEmitter;
    pub use super::basic_controls::{Button, UnipolarOsc, button, unipolar};
    pub use super::bipolar_array::{BipolarArray, bipolar_array};
    pub use super::button_array::{ButtonArray, button_array};
    pub use super::label_array::LabelArray;
    pub use super::radio_button::RadioButton;
    pub use super::unipolar_array::{UnipolarArray, unipolar_array};
    pub use super::{GroupControlMap, OscControlMessage};
    pub use crate::util::*;
}

#[cfg(test)]
mod bind_tests {
    use super::{BoundOsc, OscController};

    #[test]
    fn swap_socket_stages_the_socket_for_the_listener() {
        let (controller, _recv) = OscController::test_new();
        assert!(controller.pending_socket.lock().unwrap().is_none());

        let socket = BoundOsc::bind(0).expect("bind should succeed").socket;
        controller.swap_socket(socket);
        assert!(
            controller.pending_socket.lock().unwrap().is_some(),
            "the handed socket should be staged for the listener to adopt"
        );
    }

    #[test]
    fn bind_collision_reports_port_and_error() {
        // Bind an OS-assigned free port, then collide with it. `bind(0)` records
        // the requested port, so read the actual port from the socket.
        let held = BoundOsc::bind(0).expect("binding an ephemeral port should succeed");
        let port = held
            .socket
            .local_addr()
            .expect("bound socket should have a local address")
            .port();

        let err = BoundOsc::bind(port).expect_err("binding a held port should fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains(&port.to_string()),
            "error should name the conflicting port {port}: {msg}"
        );
    }
}
