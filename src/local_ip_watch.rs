//! Track the host's local IP so the displayed OSC address stays current.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use local_ip_address::local_ip;
use log::info;

use crate::gui_state::GuiState;

/// How often the host's primary local IP is re-checked.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Format an OSC listen address as `ip:port`, substituting `0.0.0.0` when no
/// local IP is available.
pub fn format_addr(ip: Option<IpAddr>, port: u16) -> String {
    match ip {
        Some(ip) => format!("{ip}:{port}"),
        None => format!("0.0.0.0:{port}"),
    }
}

/// The host's primary local IP, or `None` when none can be resolved.
pub fn current_ip() -> Option<IpAddr> {
    local_ip().ok()
}

/// Spawn a detached thread that refreshes the host's local IP whenever it
/// changes.
pub fn spawn(gui_state: Arc<GuiState>) {
    crate::worker::spawn("local-ip-watch", move |shutdown| {
        let mut last = **gui_state.osc_local_ip.load();
        loop {
            shutdown.sleep(POLL_INTERVAL);
            if shutdown.triggered() {
                return;
            }
            let next = current_ip();
            if next != last {
                info!("OSC local IP changed: {last:?} -> {next:?}");
                gui_state.osc_local_ip.store(next);
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
