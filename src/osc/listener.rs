//! Listen for incoming OSC messages, process them, and forward them along.

use crate::control::{ControlMessage, MetaCommand};
use crate::osc::sender::OscClientListener;
use crate::osc::{OscClientId, OscControlMessage, OscError};
use crate::worker::Shutdown;
use log::{error, warn};
use rosc::OscPacket;
use std::net::UdpSocket;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a receive blocks before the loop re-checks for a socket handoff.
const READ_TIMEOUT: Duration = Duration::from_millis(250);

/// A socket handed from a port rebind to the listener, picked up on the next
/// receive cycle. `None` when no rebind is pending.
pub(super) type PendingSocket = Arc<Mutex<Option<UdpSocket>>>;

pub struct OscListener {
    clients: OscClientListener,
    socket: UdpSocket,
    pending_socket: PendingSocket,
    buf: [u8; rosc::decoder::MTU],
    send: Sender<ControlMessage>,
}

impl OscListener {
    /// Build an OSC listener that receives on the given UDP socket, adopting any
    /// socket later placed in `pending_socket`.
    pub fn from_socket(
        clients: OscClientListener,
        socket: UdpSocket,
        pending_socket: PendingSocket,
        send: Sender<ControlMessage>,
    ) -> Self {
        apply_read_timeout(&socket);
        Self {
            clients,
            socket,
            pending_socket,
            buf: [0u8; rosc::decoder::MTU],
            send,
        }
    }

    /// Adopt a socket handed off by a port rebind, if one is waiting.
    fn swap_pending_socket(&mut self) {
        let Ok(mut slot) = self.pending_socket.lock() else {
            return;
        };
        if let Some(socket) = slot.take() {
            apply_read_timeout(&socket);
            self.socket = socket;
        }
    }

    /// Forward a control message to the show, logging if the channel has closed.
    fn emit(&self, msg: ControlMessage) {
        if let Err(e) = self.send.send(msg) {
            // Fatal: if the show's receiver is gone the listener can no longer
            // deliver any control input, so the controller is effectively dead.
            error!("OSC control channel closed: {e}");
        }
    }

    /// Recursively unpack OSC packets and send all the inner messages as control events.
    fn forward_packet(
        &mut self,
        packet: OscPacket,
        client_id: OscClientId,
    ) -> Result<(), OscError> {
        match packet {
            OscPacket::Message(m) => {
                // Set TouchOSC pages to send this message, and ignore them all here.
                if m.addr == "/ignore" {
                    return Ok(());
                }

                // If this is a deregistration message, tell the show to deregister this client.
                if m.addr == "/deregister" {
                    self.emit(ControlMessage::Meta(
                        MetaCommand::DropOscClient(client_id),
                        None,
                    ));
                    return Ok(());
                }
                let cm = OscControlMessage::new(m, client_id)?;
                self.emit(ControlMessage::Osc(cm));
            }
            OscPacket::Bundle(msgs) => {
                for subpacket in msgs.content {
                    self.forward_packet(subpacket, client_id)?;
                }
            }
        }
        Ok(())
    }

    /// Run the listener in the current thread until `shutdown` is signalled.
    pub fn run(&mut self, shutdown: Shutdown) {
        loop {
            if shutdown.triggered() {
                return;
            }
            self.swap_pending_socket();

            let (size, sender_addr) = match self.socket.recv_from(&mut self.buf) {
                Ok(v) => v,
                // The read timeout fires periodically so the loop can pick up a
                // rebind; it is not a real error.
                Err(e) if is_timeout(&e) => continue,
                Err(e) => {
                    warn!("Error receiving from OSC input: {e}");
                    continue;
                }
            };

            let packet = match rosc::decoder::decode_udp(&self.buf[..size]) {
                Ok((_, packet)) => packet,
                Err(e) => {
                    warn!("Error decoding OSC packet: {e}");
                    continue;
                }
            };
            let client_id = OscClientId(sender_addr);

            // If this is a new client, tell the show to register them.
            if !self.clients.load().contains(&client_id) {
                self.emit(ControlMessage::Meta(
                    MetaCommand::RegisterOscClient(client_id),
                    None,
                ));
            }

            if let Err(e) = self.forward_packet(packet, client_id) {
                warn!("Error unpacking/forwarding OSC packet: {e}");
            }
        }
    }
}

/// Apply the listener receive timeout to a socket, logging on failure since the
/// listener still functions without it (port rebinds just take effect later).
fn apply_read_timeout(socket: &UdpSocket) {
    if let Err(e) = socket.set_read_timeout(Some(READ_TIMEOUT)) {
        error!("failed to set OSC receive timeout: {e}");
    }
}

/// Whether a receive error is the periodic timeout rather than a real failure.
fn is_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}
