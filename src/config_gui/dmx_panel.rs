use std::time::Duration;

use eframe::egui;

use crate::control::MetaCommand;
use crate::gui_state::DmxPortStatus;
use crate::ui_util::{GuiContext, STATUS_COLORS};

pub struct DmxPortPanelState {
    available_ports: Vec<Box<dyn rust_dmx::DmxPort>>,
    scan_artnet: bool,
    artnet_timeout_secs: String,
    /// Selected port in the available list. None = "offline" selected.
    selected_port: Option<usize>,
}

impl DmxPortPanelState {
    pub fn new() -> Self {
        Self {
            available_ports: Vec::new(),
            scan_artnet: false,
            artnet_timeout_secs: "3".to_string(),
            selected_port: None,
        }
    }
}

pub(crate) struct DmxPortPanel<'a> {
    pub ctx: GuiContext<'a>,
    pub state: &'a mut DmxPortPanelState,
    pub port_status: &'a DmxPortStatus,
}

impl DmxPortPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        let artnet_valid = !self.state.scan_artnet
            || self
                .state
                .artnet_timeout_secs
                .parse::<f32>()
                .map(|v| v > 0.0)
                .unwrap_or(false);

        // Header.
        ui.heading("DMX Ports");

        // Refresh + ArtNet options.
        ui.horizontal(|ui| {
            if ui
                .add_enabled(artnet_valid, egui::Button::new("Refresh"))
                .clicked()
            {
                self.refresh_ports();
            }
            ui.checkbox(&mut self.state.scan_artnet, "Scan ArtNet");
            if self.state.scan_artnet {
                ui.label("Timeout:");
                let edit = egui::TextEdit::singleline(&mut self.state.artnet_timeout_secs)
                    .desired_width(30.0);
                ui.add(edit);
                ui.label("sec");
                if !artnet_valid {
                    ui.colored_label(STATUS_COLORS.error_text, "must be a positive number");
                }
            }
        });

        ui.separator();

        // Available ports pool.
        ui.label(format!(
            "Available Ports ({})",
            self.state.available_ports.len() + 1 // +1 for offline
        ));

        let offline_name = rust_dmx::OfflineDmxPort.to_string();
        if ui
            .selectable_label(self.state.selected_port.is_none(), &offline_name)
            .clicked()
        {
            self.state.selected_port = None;
        }

        for (i, port) in self.state.available_ports.iter().enumerate() {
            let is_selected = self.state.selected_port == Some(i);
            if ui.selectable_label(is_selected, port.to_string()).clicked() {
                self.state.selected_port = Some(i);
            }
        }

        ui.separator();

        // Universe list.
        if self.port_status.ports.is_empty() {
            ui.label("No universes configured.");
        } else {
            let selected_name = match self.state.selected_port {
                None => rust_dmx::OfflineDmxPort.to_string(),
                Some(i) => self
                    .state
                    .available_ports
                    .get(i)
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| {
                        self.state.selected_port = None;
                        rust_dmx::OfflineDmxPort.to_string()
                    }),
            };

            let mut assign_action: Option<usize> = None;

            egui::Grid::new("dmx_universe_grid")
                .striped(true)
                .show(ui, |ui| {
                    for (universe, port_name) in self.port_status.ports.iter().enumerate() {
                        let same_as_current = selected_name == *port_name;
                        if ui
                            .add_enabled(!same_as_current, egui::Button::new("Assign"))
                            .on_hover_text(format!("Assign {selected_name}"))
                            .clicked()
                        {
                            assign_action = Some(universe);
                        }

                        ui.label(format!("Universe {universe}"));
                        ui.label(port_name);
                        ui.end_row();
                    }
                });

            if let Some(universe) = assign_action {
                let port: Box<dyn rust_dmx::DmxPort> = match self.state.selected_port.take() {
                    None => Box::new(rust_dmx::OfflineDmxPort),
                    Some(i) => {
                        if i < self.state.available_ports.len() {
                            self.state.available_ports.remove(i)
                        } else {
                            Box::new(rust_dmx::OfflineDmxPort)
                        }
                    }
                };
                let _ = self
                    .ctx
                    .send_command(MetaCommand::AssignDmxPort { universe, port });
            }
        }
    }

    fn refresh_ports(&mut self) {
        let artnet_timeout = if self.state.scan_artnet {
            let secs = self
                .state
                .artnet_timeout_secs
                .parse::<f32>()
                .unwrap_or(3.0)
                .max(0.5);
            Some(Duration::from_secs_f32(secs))
        } else {
            None
        };

        match rust_dmx::available_ports(artnet_timeout) {
            Ok(ports) => {
                self.state.available_ports = ports;
                self.state.selected_port = None;
            }
            Err(e) => {
                self.ctx.report_error(format!("Port discovery failed: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::control::mock::auto_respond_client;
    use crate::ui_util::ErrorModal;
    use egui_kittest::Harness;

    #[test]
    fn render_no_universes() {
        let client = auto_respond_client();
        let status = DmxPortStatus { ports: vec![] };
        let mut error_modal = ErrorModal::default();
        let mut state = DmxPortPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            DmxPortPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                port_status: &status,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("dmx_panel_no_universes");
    }

    #[test]
    fn render_with_offline_ports() {
        let client = auto_respond_client();
        let status = DmxPortStatus {
            ports: vec!["offline".to_string(), "offline".to_string()],
        };
        let mut error_modal = ErrorModal::default();
        let mut state = DmxPortPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            DmxPortPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                port_status: &status,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("dmx_panel_offline");
    }
}
