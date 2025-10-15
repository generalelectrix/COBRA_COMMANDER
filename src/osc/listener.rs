//! Listen for incoming OSC messages, process them, and forward them along.

use crate::control::ControlMessage;
use crate::osc::{OscClientId, OscControlMessage, OscError};
use anyhow::Result;
use log::error;
use rosc::OscPacket;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc::Sender;

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
                // log::info!("Received OSC message: {:?}", m);
                // Set TouchOSC pages to send this message, and ignore them all here.
                if m.addr == "/ignore" {
                    return Ok(());
                }

                // If this is a deregistration message, tell the show to deregister this client.
                if m.addr == "/deregister" {
                    self.clients.retain(|c| *c != client_id);
                    self.send
                        .send(ControlMessage::DeregisterClient(client_id))
                        .unwrap();
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

            // If this is a new client, tell the show to register them.
            if !self.clients.contains(&client_id) {
                self.clients.push(client_id);
                self.send
                    .send(ControlMessage::RegisterClient(client_id))
                    .unwrap();
            }

            if let Err(e) = self.forward_packet(packet, client_id) {
                error!("Error unpacking/forwarding OSC packet: {e}");
            }
        }
    }
}
