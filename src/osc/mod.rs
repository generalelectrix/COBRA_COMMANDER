use crate::channel::{ChannelStateChange, ChannelStateEmitter};
use crate::control::ControlMessage;
use crate::control::EmitControlMessage;
use crate::fixture::FixtureGroupKey;
use crate::wled::EmitWledControlMessage;
use anyhow::bail;
use anyhow::Result;
use log::{error, info};
use number::{BipolarFloat, Phase, UnipolarFloat};
use rosc::{encoder, OscMessage, OscPacket, OscType};
use serde::Deserialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::Display;
use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use thiserror::Error;

use self::radio_button::RadioButton;

pub mod animation;
pub mod audio;
mod basic_controls;
pub mod channels;
pub mod clock;
mod control_message;
mod fader_array;
mod label_array;
mod radio_button;
mod register;

pub use control_message::OscControlMessage;
pub use register::prompt_osc_config;

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
}

impl OscController {
    pub fn new(
        receive_port: u16,
        send_addrs: Vec<OscClientId>,
        send: Sender<ControlMessage>,
    ) -> Result<Self> {
        let recv_addr = SocketAddr::from_str(&format!("0.0.0.0:{receive_port}"))?;
        start_listener(recv_addr, send)?;
        let response_send = start_sender(send_addrs)?;
        Ok(Self {
            send: response_send,
        })
    }

    pub fn send(&self, msg: OscControlResponse) {
        if self.send.send(msg).is_err() {
            error!("OSC send channel is disconnected.");
        }
    }
}

/// Decorate a control message emitter to inject a group into the address.
pub struct FixtureStateEmitter<'a> {
    key: &'a FixtureGroupKey,
    channel_emitter: ChannelStateEmitter<'a>,
}

impl<'a> FixtureStateEmitter<'a> {
    pub fn new(key: &'a FixtureGroupKey, channel_emitter: ChannelStateEmitter<'a>) -> Self {
        Self {
            key,
            channel_emitter,
        }
    }

    pub fn emit_channel(&self, msg: ChannelStateChange) {
        self.channel_emitter.emit(msg);
    }
}

impl<'a> EmitScopedOscMessage for FixtureStateEmitter<'a> {
    fn emit_osc(&self, msg: ScopedOscMessage) {
        let addr = format!("/{}/{}", self.key, msg.control);
        self.channel_emitter.emit_osc(OscMessage {
            addr,
            args: vec![msg.arg],
        });
    }
}

impl<'a> EmitWledControlMessage for FixtureStateEmitter<'a> {
    fn emit_wled(&self, msg: crate::wled::WledControlMessage) {
        self.channel_emitter.emit_wled(msg);
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

    pub fn add_bool<F>(&mut self, control: &str, process: F)
    where
        F: Fn(bool) -> C + 'static,
    {
        self.add_fetch_process(control, OscControlMessage::get_bool, move |v| {
            Some(process(v))
        })
    }
}

/// Forward OSC messages to the provided sender.
/// Spawns a new thread to handle listening for messages.
fn start_listener(addr: SocketAddr, send: Sender<ControlMessage>) -> Result<()> {
    let socket = UdpSocket::bind(addr)?;

    let mut buf = [0u8; rosc::decoder::MTU];

    let mut recv_packet = move || -> Result<_> {
        let (size, sender_addr) = socket.recv_from(&mut buf)?;
        let (_, packet) = rosc::decoder::decode_udp(&buf[..size])?;
        Ok((packet, OscClientId(sender_addr)))
    };

    thread::spawn(move || loop {
        let (packet, client_id) = match recv_packet() {
            Ok(msg) => msg,
            Err(e) => {
                error!("Error receiving from OSC input: {e}");
                continue;
            }
        };
        if let Err(e) = forward_packet(packet, client_id, &send) {
            error!("Error unpacking/forwarding OSC packet: {e}");
        }
    });
    Ok(())
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

/// Drain a control channel of OSC messages and send them.
/// Sends each message to every provided address, unless the talkback mode
/// says otherwise.
fn start_sender(clients: Vec<OscClientId>) -> Result<Sender<OscControlResponse>> {
    let (send, recv) = channel::<OscControlResponse>();
    let socket = UdpSocket::bind("0.0.0.0:0")?;

    thread::spawn(move || {
        let mut msg_buf = Vec::new();
        loop {
            let Ok(resp) = recv.recv() else {
                info!("OSC sender channel hung up, terminating sender thread.");
                return;
            };
            // Encode the message.
            let packet = OscPacket::Message(resp.msg);
            msg_buf.clear();
            if let Err(err) = encoder::encode_into(&packet, &mut msg_buf) {
                error!("Error encoding OSC packet {packet:?}: {err}.");
                continue;
            };
            //log::debug!("Sending OSC message: {packet:?}");
            for client in &clients {
                if resp.talkback == TalkbackMode::Off && resp.sender_id == Some(*client) {
                    continue;
                }
                if let Err(err) = socket.send_to(&msg_buf, client.addr()) {
                    error!("OSC send error to {client}: {err}.");
                }
            }
        }
    });
    Ok(send)
}

/// Recursively unpack OSC packets and send all the inner messages as control events.
fn forward_packet(
    packet: OscPacket,
    client_id: OscClientId,
    send: &Sender<ControlMessage>,
) -> Result<(), OscError> {
    match packet {
        OscPacket::Message(m) => {
            // info!("Received OSC message: {:?}", m);
            // Set TouchOSC pages to send this message, and ignore them all here.
            if m.addr == "/page" {
                return Ok(());
            }
            let cm = OscControlMessage::new(m, client_id)?;
            send.send(ControlMessage::Osc(cm)).unwrap();
        }
        OscPacket::Bundle(msgs) => {
            for subpacket in msgs.content {
                forward_packet(subpacket, client_id, send)?;
            }
        }
    }
    Ok(())
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

pub mod prelude {
    pub use super::basic_controls::{button, Button};
    pub use super::fader_array::FaderArray;
    pub use super::label_array::LabelArray;
    pub use super::FixtureStateEmitter;
    pub use super::{GroupControlMap, OscControlMessage};
    pub use crate::util::*;
}
