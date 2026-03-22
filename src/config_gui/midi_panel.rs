use eframe::egui;
use midi_harness::{PortStatus, SlotStatus};

/// Render the MIDI device slots panel.
///
/// This is a stateless rendering function — it accepts the already-loaded
/// snapshot from the caller and does not interact with SharedGuiState.
pub fn ui(ui: &mut egui::Ui, slots: &[SlotStatus]) {
    ui.heading("MIDI Devices");
    ui.separator();

    if slots.is_empty() {
        ui.label("No MIDI slots configured.");
        return;
    }

    egui::Grid::new("midi_slots_grid")
        .num_columns(4)
        .spacing([16.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            // Header row.
            ui.strong("Slot");
            ui.strong("Model");
            ui.strong("Input");
            ui.strong("Output");
            ui.end_row();

            for slot in slots {
                ui.label(&slot.name);
                ui.label(&slot.model);
                port_status_label(ui, &slot.input);
                port_status_label(ui, &slot.output);
                ui.end_row();
            }
        });
}

/// Render a single port status cell with appropriate coloring.
fn port_status_label(ui: &mut egui::Ui, status: &PortStatus) {
    match status {
        PortStatus::Unassigned => {
            ui.colored_label(egui::Color32::GRAY, "\u{2014}");
        }
        PortStatus::Disconnected { name } => {
            ui.colored_label(
                egui::Color32::from_rgb(255, 80, 80),
                format!("{name} (disconnected)"),
            );
        }
        PortStatus::Connected { name } => {
            ui.colored_label(egui::Color32::GREEN, name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn empty_slots_shows_no_configured_message() {
        let slots: Vec<SlotStatus> = vec![];
        let harness = Harness::new_ui(|ui| {
            super::ui(ui, &slots);
        });

        assert!(
            harness
                .query_by_label("No MIDI slots configured.")
                .is_some()
        );
    }

    #[test]
    fn populated_slots_shows_names_and_status() {
        let slots = vec![
            SlotStatus {
                name: "Submaster Wing 1".to_string(),
                model: "Launch Control XL".to_string(),
                input: PortStatus::Connected {
                    name: "LXCL Input".to_string(),
                },
                output: PortStatus::Disconnected {
                    name: "LXCL Output".to_string(),
                },
            },
            SlotStatus {
                name: "Clock Wing".to_string(),
                model: "CMD MM-1".to_string(),
                input: PortStatus::Unassigned,
                output: PortStatus::Unassigned,
            },
        ];
        let harness = Harness::new_ui(|ui| {
            super::ui(ui, &slots);
        });

        assert!(harness.query_by_label("Submaster Wing 1").is_some());
        assert!(harness.query_by_label("Launch Control XL").is_some());
        assert!(harness.query_by_label("Clock Wing").is_some());
        assert!(harness.query_by_label("CMD MM-1").is_some());
    }
}
