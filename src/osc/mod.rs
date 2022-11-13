use crate::fixture::aquarius::Aquarius;
use crate::fixture::color::Color;
use crate::fixture::comet::Comet;
use crate::fixture::dimmer::Dimmer;
use crate::fixture::faderboard::Faderboard;
use crate::fixture::freedom_fries::FreedomFries;
use crate::fixture::h2o::H2O;
use crate::fixture::lumasphere::Lumasphere;
use crate::fixture::radiance::Radiance;
use crate::fixture::rotosphere_q3::RotosphereQ3;
use crate::fixture::rush_wizard::RushWizard;
use crate::fixture::swarmolon::Swarmolon;
use crate::fixture::venus::Venus;
use crate::fixture::{
    ControlMessage, EmitStateChange, FixtureControlMessage, FixtureStateChange, StateChange,
};
use crate::master::MasterControls;
use control_message::OscControlMessage;
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use log::{error, info};
use number::{BipolarFloat, Phase, UnipolarFloat};
use rosc::{encoder, OscMessage, OscPacket, OscType};
use simple_error::bail;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use self::radio_button::{EnumRadioButton, RadioButton};

mod control_message;
mod fixture;
mod master;
mod radio_button;

/// Map OSC control inputs for a fixture type.
pub trait MapControls {
    /// Add OSC control mappings to the provided control map.
    fn map_controls(&self, map: &mut ControlMap<FixtureControlMessage>);
}

/// Process a state change message into OSC messages.
pub trait HandleStateChange<SC> {
    /// Convert the provided state change into OSC messages and send them.
    fn emit_state_change<S>(_sc: SC, _send: &mut S)
    where
        S: FnMut(OscMessage),
    {
    }
}

pub struct OscController {
    control_map: ControlMap<FixtureControlMessage>,
    recv: Receiver<OscControlMessage>,
    send: Sender<OscMessage>,
}

impl OscController {
    pub fn new(receive_port: u16, send_host: &str, send_port: u16) -> Result<Self, Box<dyn Error>> {
        let recv_addr = SocketAddr::from_str(&format!("0.0.0.0:{}", receive_port))?;
        let send_adr = SocketAddr::from_str(&format!("{}:{}", send_host, send_port))?;
        let control_recv = start_listener(recv_addr)?;
        let response_send = start_sender(send_adr)?;
        Ok(Self {
            control_map: ControlMap::new(),
            recv: control_recv,
            send: response_send,
        })
    }

    pub fn recv(&self, timeout: Duration) -> Result<Option<ControlMessage>, Box<dyn Error>> {
        let msg = match self.recv.recv_timeout(timeout) {
            Ok(msg) => msg,
            Err(RecvTimeoutError::Timeout) => {
                return Ok(None);
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("OSC receiver disconnected");
            }
        };
        Ok(self.control_map.handle(&msg)?.map(|m| ControlMessage {
            msg: m,
            group: msg.group.clone(),
        }))
    }

    pub fn map_controls<M: MapControls>(&mut self, fixture: &M) {
        fixture.map_controls(&mut self.control_map);
    }
}

impl EmitStateChange for OscController {
    fn emit(&mut self, sc: StateChange) {
        let send = &mut |mut msg: OscMessage| {
            if let Some(g) = sc.group.inner() {
                // If a group is set, prepend the ID to the address.
                // FIXME: would be nice to thing through this a bit and see if
                // we can avoid this allocation by somehow transparently threading
                // the group into the send call via something like constructor
                // injection.
                msg.addr = format!("/:{}{}", g, msg.addr);
            }
            let _ = self.send.send(msg);
        };
        match sc.sc {
            FixtureStateChange::Comet(sc) => Comet::emit_state_change(sc, send),
            FixtureStateChange::Lumasphere(sc) => Lumasphere::emit_state_change(sc, send),
            FixtureStateChange::Venus(sc) => Venus::emit_state_change(sc, send),
            FixtureStateChange::H2O(sc) => H2O::emit_state_change(sc, send),
            FixtureStateChange::Aquarius(sc) => Aquarius::emit_state_change(sc, send),
            FixtureStateChange::Radiance(sc) => Radiance::emit_state_change(sc, send),
            FixtureStateChange::Swarmolon(sc) => Swarmolon::emit_state_change(sc, send),
            FixtureStateChange::RotosphereQ3(sc) => RotosphereQ3::emit_state_change(sc, send),
            FixtureStateChange::FreedomFries(sc) => FreedomFries::emit_state_change(sc, send),
            FixtureStateChange::Faderboard(sc) => Faderboard::emit_state_change(sc, send),
            FixtureStateChange::RushWizard(sc) => RushWizard::emit_state_change(sc, send),
            FixtureStateChange::Color(sc) => Color::emit_state_change(sc, send),
            FixtureStateChange::Dimmer(sc) => Dimmer::emit_state_change(sc, send),
            FixtureStateChange::Master(sc) => MasterControls::emit_state_change(sc, send),
        }
    }
}

type ControlMessageCreator<C> =
    Box<dyn Fn(&OscControlMessage) -> Result<Option<C>, Box<dyn Error>>>;

pub struct ControlMap<C>(HashMap<String, ControlMessageCreator<C>>);

impl<C> ControlMap<C> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn handle(&self, msg: &OscControlMessage) -> Result<Option<C>, Box<dyn Error>> {
        let key = msg.key();
        match self.0.get(key) {
            None => {
                bail!("No control handler matched key \"{}\".", key);
            }
            Some(handler) => handler(msg),
        }
    }

    pub fn add<F>(&mut self, group: &str, control: &str, handler: F)
    where
        F: Fn(&OscControlMessage) -> Result<Option<C>, Box<dyn Error>> + 'static,
    {
        let key = format!("/{}/{}", group, control);
        match self.0.entry(key) {
            Entry::Occupied(e) => {
                let key = e.key();
                panic!("Duplicate control definition for {}", key,)
            }
            Entry::Vacant(v) => v.insert(Box::new(handler)),
        };
    }

    pub fn add_fetch_process<F, T, P>(&mut self, group: &str, control: &str, fetch: F, process: P)
    where
        F: Fn(&OscControlMessage) -> Result<T, OscError> + 'static,
        P: Fn(T) -> Option<C> + 'static,
    {
        self.add(group, control, move |v| Ok(process(fetch(v)?)))
    }

    pub fn add_unipolar<F>(&mut self, group: &str, control: &str, process: F)
    where
        F: Fn(UnipolarFloat) -> C + 'static,
    {
        self.add_fetch_process(group, control, get_unipolar, move |v| Some(process(v)))
    }

    pub fn add_bipolar<F>(&mut self, group: &str, control: &str, process: F)
    where
        F: Fn(BipolarFloat) -> C + 'static,
    {
        self.add_fetch_process(group, control, get_bipolar, move |v| Some(process(v)))
    }

    pub fn add_phase<F>(&mut self, group: &str, control: &str, process: F)
    where
        F: Fn(Phase) -> C + 'static,
    {
        self.add_fetch_process(group, control, get_phase, move |v| Some(process(v)))
    }

    pub fn add_bool<F>(&mut self, group: &str, control: &str, process: F)
    where
        F: Fn(bool) -> C + 'static,
    {
        self.add_fetch_process(group, control, get_bool, move |v| Some(process(v)))
    }

    /// Add a collection of control actions for each variant of the specified enum type.
    pub fn add_enum_handler<EnumType, Parse, Process, ParseResult>(
        &mut self,
        group: &str,
        control: &str,
        parse: Parse,
        process: Process,
    ) where
        EnumType: EnumRadioButton,
        <EnumType as FromStr>::Err: std::fmt::Display,
        Parse: Fn(&OscControlMessage) -> Result<ParseResult, OscError> + 'static,
        Process: Fn(EnumType, ParseResult) -> C + 'static,
    {
        self.add(group, control, move |m| {
            let variant: EnumType = EnumType::parse(&m)?;
            let val = parse(m)?;
            Ok(Some(process(variant, val)))
        })
    }

    pub fn add_trigger(&mut self, group: &str, control: &str, event: C)
    where
        C: Clone + 'static,
    {
        self.add_fetch_process(group, control, get_bool, move |v| {
            if v {
                Some(event.clone())
            } else {
                None
            }
        })
    }

    pub fn add_radio_button_array<F>(&mut self, rb: RadioButton, process: F)
    where
        F: Fn(usize) -> C + 'static,
    {
        self.add_fetch_process(
            rb.group,
            rb.control,
            move |m| rb.parse(m),
            move |x| Some(process(x)),
        )
    }
}

/// Forward OSC messages to the provided sender.
/// Spawns a new thread to handle listening for messages.
fn start_listener(addr: SocketAddr) -> Result<Receiver<OscControlMessage>, Box<dyn Error>> {
    let (send, recv) = unbounded();
    let socket = UdpSocket::bind(addr)?;

    let mut buf = [0u8; rosc::decoder::MTU];

    let mut recv_packet = move || -> Result<OscPacket, Box<dyn Error>> {
        let size = socket.recv(&mut buf)?;
        let (_, packet) = rosc::decoder::decode_udp(&buf[..size])?;
        Ok(packet)
    };

    thread::spawn(move || loop {
        let packet = match recv_packet() {
            Ok(packet) => packet,
            Err(e) => {
                error!("Error receiving from OSC input: {}", e);
                continue;
            }
        };
        if let Err(e) = forward_packet(packet, &send) {
            error!("Error unpacking/forwarding OSC packet: {}", e);
        }
    });
    Ok(recv)
}

/// Drain a control channel of OSC messages and send them.
/// Assumes we only have one controller.
fn start_sender(addr: SocketAddr) -> Result<Sender<OscMessage>, Box<dyn Error>> {
    let (send, recv) = unbounded();
    let socket = UdpSocket::bind("0.0.0.0:0")?;

    let send_packet = move |msg| -> Result<(), Box<dyn Error>> {
        let msg_buf = encoder::encode(&OscPacket::Message(msg))?;
        socket.send_to(&msg_buf, addr)?;
        Ok(())
    };

    thread::spawn(move || loop {
        let msg = match recv.recv() {
            Err(_) => {
                info!("OSC sender channel hung up, terminating sender thread.");
                return;
            }
            Ok(m) => m,
        };
        if let Err(e) = send_packet(msg) {
            error!("OSC send error: {}.", e);
        }
    });
    Ok(send)
}

/// Recursively unpack OSC packets and send all the inner messages as control events.
fn forward_packet(packet: OscPacket, send: &Sender<OscControlMessage>) -> Result<(), OscError> {
    match packet {
        OscPacket::Message(m) => {
            // Set TouchOSC pages to send this message, and ignore them all here.
            if m.addr == "/page" {
                return Ok(());
            }
            let cm = OscControlMessage::new(m)?;
            send.send(cm).unwrap();
        }
        OscPacket::Bundle(msgs) => {
            for subpacket in msgs.content {
                forward_packet(subpacket, send)?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct OscError {
    pub addr: String,
    pub msg: String,
}

impl Display for OscError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.addr, self.msg)
    }
}

impl Error for OscError {}

/// Get a single float argument from the provided OSC message.
fn get_float(v: &OscControlMessage) -> Result<f64, OscError> {
    match &v.arg {
        OscType::Float(v) => Ok(*v as f64),
        OscType::Double(v) => Ok(*v),
        other => Err(v.err(format!(
            "expected a single float argument but found {:?}",
            other
        ))),
    }
}

/// Get a single unipolar float argument from the provided OSC message.
fn get_unipolar(v: &OscControlMessage) -> Result<UnipolarFloat, OscError> {
    Ok(UnipolarFloat::new(get_float(v)?))
}

/// Get a single bipolar float argument from the provided OSC message.
fn get_bipolar(v: &OscControlMessage) -> Result<BipolarFloat, OscError> {
    Ok(BipolarFloat::new(get_float(v)?))
}

/// Get a single phase argument from the provided OSC message.
fn get_phase(v: &OscControlMessage) -> Result<Phase, OscError> {
    Ok(Phase::new(get_float(v)?))
}

fn quadratic(v: UnipolarFloat) -> UnipolarFloat {
    UnipolarFloat::new(v.val().powi(2))
}

/// Get a single boolean argument from the provided OSC message.
/// Coerce ints and floats to boolean values.
fn get_bool(v: &OscControlMessage) -> Result<bool, OscError> {
    let bval = match &v.arg {
        OscType::Bool(b) => *b,
        OscType::Int(i) => *i != 0,
        OscType::Float(v) => *v != 0.0,
        OscType::Double(v) => *v != 0.0,
        other => {
            return Err(v.err(format!(
                "expected a single bool argument but found {:?}",
                other
            )));
        }
    };
    Ok(bval)
}

/// A OSC message processor that ignores the message payload, returning unit.
fn ignore_payload(_: &OscControlMessage) -> Result<(), OscError> {
    Ok(())
}