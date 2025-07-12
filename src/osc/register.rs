//! Provide auto-registration of OSC controllers by listening for client messages.

use anyhow::Result;
use std::{
    collections::HashSet,
    net::{SocketAddr, UdpSocket},
    str::FromStr,
};
use tunnels_lib::prompt::prompt_bool;

use super::OscClientId;

/// Listen for a message from a OSC client.
fn register_client(receive_port: u16) -> Result<OscClientId> {
    let addr = SocketAddr::from_str(&format!("0.0.0.0:{receive_port}"))?;
    let socket = UdpSocket::bind(addr)?;
    let mut buf = [0u8; rosc::decoder::MTU];
    let (_, sender_addr) = socket.recv_from(&mut buf)?;
    Ok(OscClientId(sender_addr))
}

pub fn prompt_osc_config(receive_port: u16) -> Result<Option<Vec<OscClientId>>> {
    if !prompt_bool("Auto-register OSC controllers?")? {
        return Ok(None);
    }
    let mut clients = HashSet::new();
    loop {
        println!("Waiting for incoming message...");
        match register_client(receive_port) {
            Ok(client) => {
                println!("Registered {client}.");
                clients.insert(client);
            }
            Err(err) => {
                println!("OSC client registration error: {err:#}");
            }
        }
        if !prompt_bool("Register another OSC controller?")? {
            break;
        }
    }
    Ok(Some(clients.into_iter().collect()))
}
