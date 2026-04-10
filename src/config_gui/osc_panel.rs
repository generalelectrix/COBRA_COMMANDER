use std::path::Path;

use eframe::egui;

use crate::config::FixtureGroupConfig;
use crate::control::MetaCommand;
use crate::osc::OscClientId;
use crate::touchosc::serve::LayoutServer;
use crate::touchosc::{GroupEntry, assemble_layout};
use crate::ui_util::GuiContext;

pub struct OscPanelState {
    sync_server: Option<LayoutServer>,
}

impl OscPanelState {
    pub fn new() -> Self {
        Self { sync_server: None }
    }
}

pub(crate) struct OscPanel<'a> {
    pub ctx: GuiContext<'a>,
    pub state: &'a mut OscPanelState,
    pub listen_addr: &'a str,
    pub clients: &'a [OscClientId],
    pub groups: &'a [FixtureGroupConfig],
    pub show_file_path: &'a Path,
}

impl OscPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        ui.heading("OSC");
        ui.separator();
        ui.label(format!("Listening on {}", self.listen_addr));
        ui.add_space(8.0);

        if self.clients.is_empty() {
            ui.label("No clients connected.");
        } else {
            ui.label(format!("{} client(s) connected:", self.clients.len()));
            ui.add_space(4.0);

            let mut drop_target = None;
            for client in self.clients {
                ui.horizontal(|ui| {
                    ui.label(format!("{}", client.addr()));
                    if ui.button("Drop").clicked() {
                        drop_target = Some(*client);
                    }
                });
            }
            if let Some(client) = drop_target {
                let _ = self.ctx.send_command(MetaCommand::DropOscClient(client));
            }
        }

        // Sync server modal — shown while server is running.
        if self.state.sync_server.is_some() {
            egui::Modal::new(egui::Id::new("touchosc_sync_modal")).show(ui.ctx(), |ui| {
                ui.set_width(350.0);
                ui.heading("TouchOSC Sync");
                ui.add_space(8.0);
                ui.label(
                    "Open TouchOSC Mk1 on your device, open Layout \u{2192} Add, \
                     and select this computer to sync the template.",
                );
                ui.add_space(12.0);
                if ui.button("Stop").clicked() {
                    self.state.sync_server = None;
                    ui.close();
                }
            });
        }

        // Template buttons at the bottom.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save Template").clicked() {
                    self.save_template();
                }
                if ui.button("Send Template To Device").clicked() {
                    self.start_sync_server();
                }
            });
        });
    }

    fn show_file_stem(&self) -> &str {
        self.show_file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cobra_commander")
    }

    fn build_layout(&mut self) -> Option<crate::touchosc::Layout> {
        let entries: Vec<GroupEntry> = self
            .groups
            .iter()
            .map(|cfg| GroupEntry {
                group_name: cfg.key(),
                fixture_type: &cfg.fixture,
            })
            .collect();
        match assemble_layout(entries.into_iter()) {
            Ok(layout) => Some(layout),
            Err(e) => {
                self.ctx
                    .modal
                    .show("Template Generation Failed", format!("{e:#}"));
                None
            }
        }
    }

    fn save_template(&mut self) {
        let stem = self.show_file_stem();
        let default_name = format!("{stem}.touchosc");
        let start_dir = self.show_file_path.parent();

        let mut dialog = rfd::FileDialog::new()
            .add_filter("TouchOSC Layout", &["touchosc"])
            .set_file_name(&default_name);
        if let Some(dir) = start_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };

        let Some(layout) = self.build_layout() else {
            return;
        };
        if let Err(e) = layout.write(&path) {
            self.ctx.modal.show("Save Failed", format!("{e:#}"));
            return;
        }
        self.ctx
            .modal
            .show("Template Saved", format!("Saved to {}", path.display()));
    }

    fn start_sync_server(&mut self) {
        let Some(layout) = self.build_layout() else {
            return;
        };
        let xml = layout.to_xml();
        let layout_name = self.show_file_stem().to_string();
        match LayoutServer::start(layout_name, &xml) {
            Ok(server) => {
                self.state.sync_server = Some(server);
            }
            Err(e) => {
                self.ctx.modal.show("Sync Server Failed", format!("{e:#}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::mock::auto_respond_client;
    use crate::ui_util::MessageModal;
    use egui_kittest::Harness;
    use std::path::PathBuf;

    #[test]
    fn render_no_clients() {
        let client = auto_respond_client();
        let mut modal = MessageModal::default();
        let clients: Vec<OscClientId> = vec![];
        let mut state = OscPanelState::new();
        let groups: Vec<FixtureGroupConfig> = vec![];
        let show_path = PathBuf::from("/tmp/test_show.cobra");
        let mut harness = Harness::new_ui(|ui| {
            OscPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut state,
                listen_addr: "192.168.1.42:8000",
                clients: &clients,
                groups: &groups,
                show_file_path: &show_path,
            }
            .ui(ui);
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
        let mut state = OscPanelState::new();
        let groups: Vec<FixtureGroupConfig> = vec![];
        let show_path = PathBuf::from("/tmp/test_show.cobra");
        let mut harness = Harness::new_ui(|ui| {
            OscPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut state,
                listen_addr: "192.168.1.42:8000",
                clients: &clients,
                groups: &groups,
                show_file_path: &show_path,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("osc_panel_with_clients");
    }
}
