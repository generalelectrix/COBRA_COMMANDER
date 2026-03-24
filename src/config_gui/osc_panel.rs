use eframe::egui;

use crate::control::MetaCommand;
use crate::osc::OscClientId;
use crate::ui_util::GuiContext;

/// Render the OSC clients panel.
pub fn ui(ui: &mut egui::Ui, ctx: &mut GuiContext<'_>, listen_addr: &str, clients: &[OscClientId]) {
    ui.heading("OSC");
    ui.separator();
    ui.label(format!("Listening on {listen_addr}"));
    ui.add_space(8.0);

    if clients.is_empty() {
        ui.label("No clients connected.");
        return;
    }

    ui.label(format!("{} client(s) connected:", clients.len()));
    ui.add_space(4.0);

    let mut drop_target = None;
    for client in clients {
        ui.horizontal(|ui| {
            ui.label(format!("{}", client.addr()));
            if ui.button("Drop").clicked() {
                drop_target = Some(*client);
            }
        });
    }
    if let Some(client) = drop_target {
        let _ = ctx.send_command(MetaCommand::DropOscClient(client));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::mock::auto_respond_client;
    use crate::ui_util::MessageModal;
    use egui_kittest::Harness;

    #[test]
    fn render_no_clients() {
        let client = auto_respond_client();
        let mut modal = MessageModal::default();
        let clients: Vec<OscClientId> = vec![];
        let mut harness = Harness::new_ui(|ui| {
            let mut ctx = GuiContext {
                modal: &mut modal,
                client: &client,
            };
            super::ui(ui, &mut ctx, "192.168.1.42:8000", &clients);
        });
        harness.run();
        harness.snapshot("osc_panel_empty");
    }

    #[test]
    fn render_with_clients() {
        use std::net::SocketAddr;
        use std::str::FromStr;

        let client = auto_respond_client();
        let mut modal = MessageModal::default();
        let clients: Vec<OscClientId> = vec![
            OscClientId::from_addr(SocketAddr::from_str("192.168.1.10:9000").unwrap()),
            OscClientId::from_addr(SocketAddr::from_str("192.168.1.20:9000").unwrap()),
            OscClientId::from_addr(SocketAddr::from_str("10.0.0.5:8001").unwrap()),
        ];
        let mut harness = Harness::new_ui(|ui| {
            let mut ctx = GuiContext {
                modal: &mut modal,
                client: &client,
            };
            super::ui(ui, &mut ctx, "192.168.1.42:8000", &clients);
        });
        harness.run();
        harness.snapshot("osc_panel_with_clients");
    }
}
