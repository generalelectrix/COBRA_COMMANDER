//! Listen for incoming OSC messages, process them, and forward them along.

use crate::channel::{ChannelStateChange, ChannelStateEmitter};
use crate::control::ControlMessage;
use crate::control::EmitControlMessage;
use crate::fixture::FixtureGroupKey;
use crate::osc::sender::{OscSender, OscSenderCommand};
use crate::osc::{OscClientId, OscControlMessage, OscError};
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

pub struct OscListener {
    clients: Vec<OscClientId>,
    socket: UdpSocket,
    buf: [u8; rosc::decoder::MTU],
    send: Sender<ControlMessage>,
}

impl OscListener {
    /// Initialize OSC listener.
    pub fn new(
        initial_clients: Vec<OscClientId>,
        addr: SocketAddr,
        send: Sender<ControlMessage>,
    ) -> Result<Self> {
        let socket = UdpSocket::bind(addr)?;

        Ok(Self {
            clients: initial_clients,
            socket,
            buf: [0u8; rosc::decoder::MTU],
            send,
        })
    }

    fn recv_packet(&mut self) -> Result<(OscPacket, OscClientId)> {
        let (size, sender_addr) = self.socket.recv_from(&mut self.buf)?;
        let (_, packet) = rosc::decoder::decode_udp(&self.buf[..size])?;
        Ok((packet, OscClientId(sender_addr)))
    }

    /// Recursively unpack OSC packets and send all the inner messages as control events.
    fn forward_packet(
        &mut self,
        packet: OscPacket,
        client_id: OscClientId,
    ) -> Result<(), OscError> {
        match packet {
            OscPacket::Message(m) => {
                // info!("Received OSC message: {:?}", m);
                // Set TouchOSC pages to send this message, and ignore them all here.
                if m.addr == "/ignore" {
                    return Ok(());
                }
                let cm = OscControlMessage::new(m, client_id)?;
                self.send.send(ControlMessage::Osc(cm)).unwrap();
            }
            OscPacket::Bundle(msgs) => {
                for subpacket in msgs.content {
                    self.forward_packet(subpacket, client_id)?;
                }
            }
        }
        Ok(())
    }

    /// Run the listener in the current thread until the show hangs up.
    pub fn run(&mut self) {
        loop {
            let (packet, client_id) = match self.recv_packet() {
                Ok(msg) => msg,
                Err(e) => {
                    error!("Error receiving from OSC input: {e}");
                    continue;
                }
            };
            if let Err(e) = self.forward_packet(packet, client_id) {
                error!("Error unpacking/forwarding OSC packet: {e}");
            }
        }
    }
}
