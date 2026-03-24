use std::time::Duration;

use eframe::egui;

use crate::control::MetaCommand;
use crate::gui_state::DmxPortStatus;
use crate::ui_util::GuiContext;

pub struct DmxPortPanelState {
    available_ports: Vec<Box<dyn rust_dmx::DmxPort>>,
    scan_artnet: bool,
    artnet_timeout_secs: String,
    /// Universe currently showing the assign picker, if any.
    assigning_universe: Option<usize>,
}

impl DmxPortPanelState {
    pub fn new() -> Self {
        Self {
            available_ports: Vec::new(),
            scan_artnet: false,
            artnet_timeout_secs: "3".to_string(),
            assigning_universe: None,
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
        ui.horizontal(|ui| {
            ui.heading("DMX Ports");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(artnet_valid, egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.refresh_ports(ui);
                }
            });
        });

        // ArtNet options.
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.state.scan_artnet, "Scan ArtNet");
            if self.state.scan_artnet {
                ui.label("Timeout:");
                let edit = egui::TextEdit::singleline(&mut self.state.artnet_timeout_secs)
                    .desired_width(30.0);
                ui.add(edit);
                ui.label("sec");
                if !artnet_valid {
                    ui.colored_label(egui::Color32::RED, "must be a positive number");
                }
            }
        });

        ui.separator();

        // Universe list.
        if self.port_status.ports.is_empty() {
            ui.label("No universes configured.");
        } else {
            // Collect assignment action to apply after rendering.
            let mut assign_action: Option<(usize, Box<dyn rust_dmx::DmxPort>)> = None;
            let mut assign_offline: Option<usize> = None;

            egui::Grid::new("dmx_universe_grid")
                .striped(true)
                .show(ui, |ui| {
                    for (universe, port_name) in self.port_status.ports.iter().enumerate() {
                        ui.label(format!("Universe {universe}"));
                        ui.label(port_name);

                        if self.state.assigning_universe == Some(universe) {
                            // Show picker.
                            if ui.button("offline").clicked() {
                                assign_offline = Some(universe);
                            }
                            for i in 0..self.state.available_ports.len() {
                                let name = self.state.available_ports[i].to_string();
                                if ui.button(&name).clicked() {
                                    let port = self.state.available_ports.remove(i);
                                    assign_action = Some((universe, port));
                                    break;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.state.assigning_universe = None;
                            }
                        } else if ui.button("Assign").clicked() {
                            self.state.assigning_universe = Some(universe);
                        }

                        ui.end_row();
                    }
                });

            // Apply assignment.
            if let Some((universe, port)) = assign_action {
                let _ = self
                    .ctx
                    .send_command(MetaCommand::AssignDmxPort { universe, port });
                self.state.assigning_universe = None;
            }
            if let Some(universe) = assign_offline {
                let port = Box::new(rust_dmx::OfflineDmxPort) as Box<dyn rust_dmx::DmxPort>;
                let _ = self
                    .ctx
                    .send_command(MetaCommand::AssignDmxPort { universe, port });
                self.state.assigning_universe = None;
            }
        }

        // Available ports pool.
        ui.separator();
        ui.label(format!(
            "Available Ports ({})",
            self.state.available_ports.len()
        ));
        if self.state.available_ports.is_empty() {
            ui.colored_label(
                ui.style().visuals.text_color().gamma_multiply(0.5),
                "None — click Refresh to discover ports.",
            );
        } else {
            for port in &self.state.available_ports {
                ui.label(port.to_string());
            }
        }
    }

    fn refresh_ports(&mut self, ui: &mut egui::Ui) {
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

        // Show a brief "scanning" state — egui will repaint after this frame.
        ui.ctx().request_repaint();

        match rust_dmx::available_ports(artnet_timeout) {
            Ok(ports) => {
                self.state.available_ports = ports;
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
