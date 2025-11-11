//! Manage sending of OSC messages to clients.
use anyhow::Result;
use std::{
    net::UdpSocket,
    sync::mpsc::{Receiver, Sender, channel},
};

use log::{error, info};
use rosc::{OscPacket, encoder};

use crate::osc::{OscClientId, OscControlResponse, TalkbackMode};

/// Drain a control channel of OSC messages and send them.
/// Sends each message to every provided address, unless the talkback mode
/// says otherwise.
pub struct OscSender {
    socket: UdpSocket,
    clients: Vec<OscClientId>,
    msg_buf: Vec<u8>,
    recv: Receiver<OscSenderCommand>,
}

impl OscSender {
    /// Initialize a sender; return the channel to communicate with it.
    pub fn new(clients: Vec<OscClientId>) -> Result<(Self, Sender<OscSenderCommand>)> {
        let (send, recv) = channel::<OscSenderCommand>();
        let socket = UdpSocket::bind("0.0.0.0:0")?;

        Ok((
            Self {
                socket,
                clients,
                msg_buf: vec![],
                recv,
            },
            send,
        ))
    }

    /// Send an OSC message to all clients.
    ///
    /// Error conditions are logged.
    fn send(&mut self, resp: OscControlResponse) {
        // Encode the message.
        let packet = OscPacket::Message(resp.msg);
        self.msg_buf.clear();
        if let Err(err) = encoder::encode_into(&packet, &mut self.msg_buf) {
            error!("Error encoding OSC packet {packet:?}: {err}.");
            return;
        };
        //log::debug!("Sending OSC message: {packet:?}");
        for client in &self.clients {
            if resp.talkback == TalkbackMode::Off && resp.sender_id == Some(*client) {
                continue;
            }
            if let Err(err) = self.socket.send_to(&self.msg_buf, client.addr()) {
                error!("OSC send error to {client}: {err}.");
            }
        }
    }

    /// Register a client.
    fn register(&mut self, client_id: OscClientId) {
        if self.clients.contains(&client_id) {
            return;
        }
        self.clients.push(client_id);
    }

    /// Deregister a client.
    fn deregister(&mut self, client_id: OscClientId) {
        self.clients.retain(|c| *c != client_id);
    }

    /// Run the sender in the current thread until the channel hangs up.
    pub fn run(&mut self) {
        use OscSenderCommand::*;
        loop {
            let Ok(msg) = self.recv.recv() else {
                info!("OSC sender channel hung up, terminating sender.");
                return;
            };
            match msg {
                SendMessage(msg) => {
                    self.send(msg);
                }
                Register(c) => {
                    self.register(c);
                }
                Deregister(c) => {
                    self.deregister(c);
                }
            }
        }
    }
}

pub enum OscSenderCommand {
    /// Send an OSC message to clients.
    SendMessage(OscControlResponse),
    /// Add a client to the listeners.
    Register(OscClientId),
    /// Remove a client from the listeners.
    Deregister(OscClientId),
}
