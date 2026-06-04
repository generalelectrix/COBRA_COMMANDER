//! Keep the displayed OSC listen address in sync with the host's local IP.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use local_ip_address::local_ip;
use log::info;

use crate::gui_state::GuiState;

/// How often the host's primary local IP is re-checked.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Format an OSC listen address as `ip:port`, substituting `0.0.0.0` when no
/// local IP is available.
fn format_addr(ip: Option<IpAddr>, port: u16) -> String {
    match ip {
        Some(ip) => format!("{ip}:{port}"),
        None => format!("0.0.0.0:{port}"),
    }
}

/// The current OSC listen address for the given receive port, derived from the
/// host's primary local IP.
pub fn listen_addr(port: u16) -> String {
    format_addr(local_ip().ok(), port)
}

/// Spawn a detached thread that refreshes the displayed OSC listen address
/// whenever the host's primary local IP changes.
pub fn spawn(gui_state: Arc<GuiState>) {
    std::thread::spawn(move || {
        let mut last = gui_state.osc_listen_addr.load().as_str().to_owned();
        loop {
            std::thread::sleep(POLL_INTERVAL);
            let port = gui_state.osc_receive_port.load(Ordering::Relaxed);
            let next = listen_addr(port);
            if next != last {
                info!("OSC listen address changed: {last} -> {next}");
                gui_state.osc_listen_addr.store(next.clone());
                last = next;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn format_addr_uses_ip_when_present_and_falls_back_otherwise() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42));
        assert_eq!(format_addr(Some(ip), 8000), "192.168.1.42:8000");
        assert_eq!(format_addr(None, 8000), "0.0.0.0:8000");
    }
}
