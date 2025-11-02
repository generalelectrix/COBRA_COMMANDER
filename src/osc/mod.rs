use crate::channel::{ChannelStateChange, ChannelStateEmitter};
use crate::config::FixtureGroupKey;
use crate::control::ControlMessage;
use crate::control::EmitControlMessage;
use crate::midi::{EmitMidiAnimationMessage, EmitMidiMasterMessage};
use crate::osc::listener::OscListener;
use crate::osc::sender::{OscSender, OscSenderCommand};
use anyhow::Result;
use anyhow::{bail, Context};
use log::error;
use number::{BipolarFloat, Phase, UnipolarFloat};
use rosc::{OscMessage, OscType};
use serde::Deserialize;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::Display;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::mpsc::Sender;
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
mod radio_button;
mod register;
mod sender;
mod unipolar_array;

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
    send: Sender<OscSenderCommand>,
}

impl OscController {
    pub fn new(
        receive_port: u16,
        send_addrs: Vec<OscClientId>,
        send: Sender<ControlMessage>,
    ) -> Result<Self> {
        let recv_addr = SocketAddr::from_str(&format!("0.0.0.0:{receive_port}"))?;

        let mut listener = OscListener::new(send_addrs.clone(), recv_addr, send)
            .context("failed to start OSC listener")?;

        thread::spawn(move || {
            listener.run();
        });

        let (mut sender, response_send) =
            OscSender::new(send_addrs).context("failed to start OSC sender")?;

        thread::spawn(move || {
            sender.run();
        });

        Ok(Self {
            send: response_send,
        })
    }

    /// Send an OSC message to all clients.
    pub fn send(&self, msg: OscControlResponse) {
        if self.send.send(OscSenderCommand::SendMessage(msg)).is_err() {
            error!("OSC send channel is disconnected.");
        }
    }

    /// Register an OSC client.
    pub fn register(&self, client_id: OscClientId) {
        if self
            .send
            .send(OscSenderCommand::Register(client_id))
            .is_err()
        {
            error!("OSC send channel is disconnected.");
        }
    }

    /// Deregister an OSC client.
    pub fn deregister(&self, client_id: OscClientId) {
        if self
            .send
            .send(OscSenderCommand::Deregister(client_id))
            .is_err()
        {
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

pub mod prelude {
    pub use super::basic_controls::{button, unipolar, Button, UnipolarOsc};
    pub use super::bipolar_array::{bipolar_array, BipolarArray};
    pub use super::button_array::{button_array, ButtonArray};
    pub use super::label_array::LabelArray;
    pub use super::radio_button::RadioButton;
    pub use super::unipolar_array::{unipolar_array, UnipolarArray};
    pub use super::FixtureStateEmitter;
    pub use super::{GroupControlMap, OscControlMessage};
    pub use crate::util::*;
}
