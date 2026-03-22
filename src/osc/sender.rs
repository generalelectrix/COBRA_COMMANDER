//! Manage sending of OSC messages to clients.
use anyhow::Result;
use std::{
    net::UdpSocket,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
};

use arc_swap::ArcSwap;
use log::{error, info};
use rosc::{OscPacket, encoder};

use crate::osc::{OscClientId, OscControlResponse, TalkbackMode};

// TODO: The sender now maintains two representations of the client list: its local
// Vec<OscClientId> (used for sending) and the shared ArcSwap (published for the GUI).
// Register/Deregister commands update the local vec then publish to the ArcSwap.
// A future simplification: replace both with a single shared ArcSwap that the show
// thread writes to directly (skipping OscSenderCommand::Register/Deregister entirely).
// The sender would just load() from the ArcSwap when sending. Simpler, though less explicit.

/// Write-only handle to the shared client list. Not Clone — only the sender thread holds one.
pub struct OscClientWriter(Arc<ArcSwap<Vec<OscClientId>>>);

/// Read-only handle to the shared client list.
#[derive(Clone)]
pub struct OscClientReader(Arc<ArcSwap<Vec<OscClientId>>>);

impl OscClientWriter {
    pub fn new(initial: Vec<OscClientId>) -> (Self, OscClientReader) {
        let shared = Arc::new(ArcSwap::from_pointee(initial));
        (Self(Arc::clone(&shared)), OscClientReader(shared))
    }

    fn publish(&self, clients: &[OscClientId]) {
        self.0.store(Arc::new(clients.to_vec()));
    }
}

impl OscClientReader {
    pub fn load(&self) -> arc_swap::Guard<Arc<Vec<OscClientId>>> {
        self.0.load()
    }
}

/// Drain a control channel of OSC messages and send them.
/// Sends each message to every provided address, unless the talkback mode
/// says otherwise.
pub struct OscSender {
    socket: UdpSocket,
    clients: Vec<OscClientId>,
    client_writer: OscClientWriter,
    msg_buf: Vec<u8>,
    recv: Receiver<OscSenderCommand>,
}

impl OscSender {
    /// Initialize a sender; return the command channel and a reader for the client list.
    pub fn new(
        clients: Vec<OscClientId>,
    ) -> Result<(Self, Sender<OscSenderCommand>, OscClientReader)> {
        let (send, recv) = channel::<OscSenderCommand>();
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        let (client_writer, client_reader) = OscClientWriter::new(clients.clone());

        Ok((
            Self {
                socket,
                clients,
                client_writer,
                msg_buf: vec![],
                recv,
            },
            send,
            client_reader,
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
        self.client_writer.publish(&self.clients);
    }

    /// Deregister a client.
    fn deregister(&mut self, client_id: OscClientId) {
        self.clients.retain(|c| *c != client_id);
        self.client_writer.publish(&self.clients);
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
