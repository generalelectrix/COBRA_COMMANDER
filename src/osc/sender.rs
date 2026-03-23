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

/// Mutable handle to the shared client list. Not Clone — only the OscController holds one.
pub struct OscClientManager(Arc<ArcSwap<Vec<OscClientId>>>);

/// Read-only handle to the shared client list. Used by the listener, sender, and GUI.
#[derive(Clone)]
pub struct OscClientListener(Arc<ArcSwap<Vec<OscClientId>>>);

impl OscClientManager {
    pub fn new(initial: Vec<OscClientId>) -> (Self, OscClientListener) {
        let shared = Arc::new(ArcSwap::from_pointee(initial));
        (Self(Arc::clone(&shared)), OscClientListener(shared))
    }

    /// Add a client if not already present.
    pub fn register(&self, client_id: OscClientId) {
        let current = self.0.load();
        if current.contains(&client_id) {
            return;
        }
        let mut next = (**current).clone();
        next.push(client_id);
        self.0.store(Arc::new(next));
    }

    /// Get a read-only listener handle backed by the same shared state.
    pub fn listener(&self) -> OscClientListener {
        OscClientListener(Arc::clone(&self.0))
    }

    /// Remove a client.
    pub fn deregister(&self, client_id: OscClientId) {
        let current = self.0.load();
        if !current.contains(&client_id) {
            return;
        }
        let mut next = (**current).clone();
        next.retain(|c| *c != client_id);
        self.0.store(Arc::new(next));
    }
}

impl OscClientListener {
    pub fn load(&self) -> arc_swap::Guard<Arc<Vec<OscClientId>>> {
        self.0.load()
    }
}

/// Drain a control channel of OSC messages and send them.
/// Sends each message to every provided address, unless the talkback mode
/// says otherwise.
pub struct OscSender {
    socket: UdpSocket,
    clients: OscClientListener,
    msg_buf: Vec<u8>,
    recv: Receiver<OscControlResponse>,
}

impl OscSender {
    /// Initialize a sender; return the channel for submitting messages.
    pub fn new(clients: OscClientListener) -> Result<(Self, Sender<OscControlResponse>)> {
        let (send, recv) = channel::<OscControlResponse>();
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
        let clients = self.clients.load();
        for client in clients.iter() {
            if resp.talkback == TalkbackMode::Off && resp.sender_id == Some(*client) {
                continue;
            }
            if let Err(err) = self.socket.send_to(&self.msg_buf, client.addr()) {
                error!("OSC send error to {client}: {err}.");
            }
        }
    }

    /// Run the sender in the current thread until the channel hangs up.
    pub fn run(&mut self) {
        loop {
            let Ok(msg) = self.recv.recv() else {
                info!("OSC sender channel hung up, terminating sender.");
                return;
            };
            self.send(msg);
        }
    }
}
