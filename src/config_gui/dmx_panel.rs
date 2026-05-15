use std::time::Duration;

use eframe::egui;

use crate::control::MetaCommand;
use crate::gui_state::DmxPortStatus;
use crate::ui_util::GuiContext;
use gui_common::STATUS_COLORS;

/// Per-universe mutable UI state for the DMX port panel.
///
/// Resized to match the snapshot at the top of each render; new rows start
/// at `Default::default()`.
#[derive(Default)]
struct UniversePanelState {
    /// Editable buffer for the FPS text entry.
    ///
    /// Reconciled with the snapshot when the field is not focused.
    framerate_text: String,
}

pub struct DmxPortPanelState {
    available_ports: Vec<Box<dyn rust_dmx::DmxPort>>,
    scan_artnet: bool,
    artnet_timeout_secs: String,
    /// Selected port in the available list. None = "offline" selected.
    selected_port: Option<usize>,
    /// One entry per universe; resized to match the snapshot each frame.
    universes: Vec<UniversePanelState>,
}

impl DmxPortPanelState {
    pub fn new() -> Self {
        Self {
            available_ports: Vec::new(),
            scan_artnet: false,
            artnet_timeout_secs: "3".to_string(),
            selected_port: None,
            universes: Vec::new(),
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
            let mut framerate_action: Option<(usize, u8)> = None;
            let mut framerate_error: Option<String> = None;

            // Keep one UI-state slot per universe; new rows default-init.
            self.state
                .universes
                .resize_with(self.port_status.ports.len(), UniversePanelState::default);

            egui::Grid::new("dmx_universe_grid")
                .striped(true)
                .show(ui, |ui| {
                    for (universe, port_info) in self.port_status.ports.iter().enumerate() {
                        let same_as_current = selected_name == port_info.name;
                        if ui
                            .add_enabled(!same_as_current, egui::Button::new("Assign"))
                            .on_hover_text(format!("Assign {selected_name}"))
                            .clicked()
                        {
                            assign_action = Some(universe);
                        }

                        ui.label(format!("Universe {universe}"));
                        ui.label(&port_info.name);

                        if let Some(current_fps) = port_info.framerate {
                            let Some(row) = self.state.universes.get_mut(universe) else {
                                ui.end_row();
                                continue;
                            };
                            let edit = egui::TextEdit::singleline(&mut row.framerate_text)
                                .desired_width(40.0);
                            let response = ui.add(edit);
                            // Capture commit *before* syncing from snapshot —
                            // on the lost-focus frame, has_focus is already
                            // false, so an unguarded snapshot sync would
                            // overwrite the user's input before we read it.
                            if response.lost_focus() {
                                match row.framerate_text.parse::<u8>() {
                                    Ok(fps) if fps > 0 => {
                                        framerate_action = Some((universe, fps));
                                    }
                                    _ => {
                                        framerate_error = Some(format!(
                                            "invalid FPS \"{}\" (expected 1..=255)",
                                            row.framerate_text
                                        ));
                                        row.framerate_text = current_fps.to_string();
                                    }
                                }
                            } else if !response.has_focus() {
                                let displayed = current_fps.to_string();
                                if row.framerate_text != displayed {
                                    row.framerate_text = displayed;
                                }
                            }
                            ui.label("fps");
                        }

                        ui.end_row();
                    }
                });

            if let Some(msg) = framerate_error {
                self.ctx.report_error(msg);
            }

            if let Some((universe, framerate)) = framerate_action {
                match self.ctx.send_command(MetaCommand::SetDmxPortFramerate {
                    universe,
                    framerate,
                }) {
                    Ok(()) => {
                        self.ctx.report_info(
                            "Framerate updated",
                            format!("Universe {universe} set to {framerate} fps."),
                        );
                    }
                    Err(e) => {
                        self.ctx.report_error(format!("{e:#}"));
                        // Revert text on failure so the user sees the still-current
                        // value on the next frame even if the field re-focuses
                        // before the next snapshot arrives.
                        if let Some(row) = self.state.universes.get_mut(universe)
                            && let Some(current) = self
                                .port_status
                                .ports
                                .get(universe)
                                .and_then(|p| p.framerate)
                        {
                            row.framerate_text = current.to_string();
                        }
                    }
                }
            }

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
    use crate::control::mock::{auto_respond_client, recording_client};
    use crate::gui_state::DmxPortInfo;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};
    use gui_common::MessageModal;

    fn offline_info() -> DmxPortInfo {
        DmxPortInfo {
            name: "offline".to_string(),
            framerate: None,
        }
    }

    #[test]
    fn render_no_universes() {
        let client = auto_respond_client();
        let status = DmxPortStatus { ports: vec![] };
        let mut modal = MessageModal::default();
        let mut state = DmxPortPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            DmxPortPanel {
                ctx: GuiContext {
                    modal: &mut modal,
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
            ports: vec![offline_info(), offline_info()],
        };
        let mut modal = MessageModal::default();
        let mut state = DmxPortPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            DmxPortPanel {
                ctx: GuiContext {
                    modal: &mut modal,
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

    #[test]
    fn fps_commit_sends_typed_value_not_snapshot() {
        // Regression: previously, the snapshot-sync branch ran before the
        // lost_focus branch on the commit frame, overwriting the user's input
        // with the snapshot value — so every commit re-sent the current FPS,
        // not the value the user typed.
        let (client, log) = recording_client();
        let status = DmxPortStatus {
            ports: vec![DmxPortInfo {
                name: "mock-port".to_string(),
                framerate: Some(40),
            }],
        };
        let mut modal = MessageModal::default();

        let mut harness = Harness::new_ui_state(
            |ui, state: &mut DmxPortPanelState| {
                DmxPortPanel {
                    ctx: GuiContext {
                        modal: &mut modal,
                        client: &client,
                    },
                    state,
                    port_status: &status,
                }
                .ui(ui);
            },
            DmxPortPanelState::new(),
        );

        // First frame: the not-focused sync populates the buffer with "40".
        harness.run();

        // Locate the FPS field. Both the TextInput container and its inner
        // TextRun report value="40"; the container is first in document order.
        let fields: Vec<_> = harness.get_all_by_value("40").collect();
        fields
            .first()
            .unwrap_or_else(|| panic!("no field with value 40 found"))
            .focus();
        harness.run();

        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.run();
        let fields: Vec<_> = harness.get_all_by_value("40").collect();
        fields
            .first()
            .unwrap_or_else(|| panic!("no field with value 40 found after select-all"))
            .type_text("30");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();

        let log = log.lock().unwrap();
        let last = log
            .last()
            .unwrap_or_else(|| panic!("no command sent; log: {log:?}"));
        assert_eq!(
            last, "SetDmxPortFramerate(0, 30 fps)",
            "expected typed value 30, got: {last} (full log: {log:?})",
        );
    }

    #[test]
    fn render_with_framerate_capable_port() {
        let client = auto_respond_client();
        let status = DmxPortStatus {
            ports: vec![
                offline_info(),
                DmxPortInfo {
                    name: "mock-port".to_string(),
                    framerate: Some(40),
                },
            ],
        };
        let mut modal = MessageModal::default();
        let mut state = DmxPortPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            DmxPortPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut state,
                port_status: &status,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("dmx_panel_with_framerate");
    }
}
