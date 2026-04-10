use std::sync::Arc;
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use log::{error, info};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tiny_http::{Header, Response, Server};

use super::model::TouchOscXml;

const PORT: u16 = 9658;
const SERVICE_TYPE: &str = "_touchosceditor._tcp.local.";

/// An on-demand HTTP server that emulates the TouchOSC Mk1 editor's layout
/// sync protocol. Advertises itself via mDNS as `_touchosceditor._tcp` and
/// serves the layout on any GET request.
///
/// The TouchOSC Mk1 app expects raw XML (not the ZIP container), so the
/// server extracts `index.xml` from the provided `.touchosc` bytes on startup.
pub struct LayoutServer {
    http_server: Arc<Server>,
    thread: Option<JoinHandle<()>>,
    mdns: ServiceDaemon,
    service_fullname: String,
}

impl LayoutServer {
    /// Start serving the given layout.
    ///
    /// `layout_name` is the name presented to clients (appears as the filename
    /// in the download).
    ///
    /// Registers a `_touchosceditor._tcp` mDNS service and spawns a thread
    /// to handle HTTP requests. Returns immediately.
    pub fn start(layout_name: String, xml: &TouchOscXml) -> Result<Self> {
        let layout_xml = xml.0.clone();

        let http_server = Arc::new(
            Server::http(format!("0.0.0.0:{PORT}"))
                .map_err(|e| anyhow::anyhow!("{e}"))
                .context("failed to start HTTP server")?,
        );

        // Use the short hostname as the mDNS instance name, matching how the
        // real TouchOSC editor advertises itself.
        let mdns = ServiceDaemon::new().context("failed to start mDNS daemon")?;
        let full_hostname = gethostname::gethostname();
        let full = full_hostname.to_string_lossy();
        let short_hostname = full.split('.').next().unwrap_or(&full);
        let hostname = format!("{short_hostname}.local.");
        let local_ip = local_ip_address::local_ip().context("failed to get local IP")?;
        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            short_hostname,
            &hostname,
            local_ip,
            PORT,
            None,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create mDNS service info")?;
        let service_fullname = service_info.get_fullname().to_string();
        mdns.register(service_info)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to register mDNS service")?;

        info!("TouchOSC layout server started on port {PORT}, advertising as {short_hostname}");

        let server = Arc::clone(&http_server);
        let thread = thread::spawn(move || {
            serve_loop(&server, &layout_xml, &layout_name);
        });

        Ok(Self {
            http_server,
            thread: Some(thread),
            mdns,
            service_fullname,
        })
    }

    /// Stop the server, deregister mDNS, and wait for the thread to finish.
    #[expect(unused)]
    pub fn stop(mut self) -> Result<()> {
        info!("stopping TouchOSC layout server");
        self.shutdown();
        Ok(())
    }

    fn shutdown(&mut self) {
        self.http_server.unblock();
        let _ = self.mdns.unregister(&self.service_fullname);
        let _ = self.mdns.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for LayoutServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve_loop(server: &Server, layout_xml: &[u8], layout_name: &str) {
    let content_type: Header = "Content-Type: application/touchosc"
        .parse()
        .expect("valid header");
    let content_disposition: Header =
        format!("Content-Disposition: attachment; filename=\"{layout_name}.touchosc\"")
            .parse()
            .expect("valid header");

    for request in server.incoming_requests() {
        info!(
            "TouchOSC sync request from {}",
            request
                .remote_addr()
                .map_or("unknown".to_string(), |a| a.to_string()),
        );

        let response = Response::from_data(layout_xml)
            .with_header(content_type.clone())
            .with_header(content_disposition.clone());

        if let Err(e) = request.respond(response) {
            error!("failed to respond to TouchOSC sync request: {e}");
        }
    }
}
