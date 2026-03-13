use eframe::egui;
use midi_harness::{DeviceKind, MidiPortSpec, PortStatus, SlotStatus};
use tunnels::midi::list_ports;

use crate::control::MetaCommand;
use crate::ui_util::{GuiContext, STATUS_COLORS};

pub struct MidiPanelState {
    input_ports: Vec<MidiPortSpec>,
    output_ports: Vec<MidiPortSpec>,
}

impl MidiPanelState {
    pub fn new() -> Self {
        let (input_ports, output_ports) = list_ports().unwrap_or_default();
        Self {
            input_ports,
            output_ports,
        }
    }

    fn refresh_ports(&mut self) {
        if let Ok((inputs, outputs)) = list_ports() {
            self.input_ports = inputs;
            self.output_ports = outputs;
        }
    }

    fn ports_for_kind(&self, kind: DeviceKind) -> &[MidiPortSpec] {
        match kind {
            DeviceKind::Input => &self.input_ports,
            DeviceKind::Output => &self.output_ports,
        }
    }
}

pub(crate) struct MidiPanel<'a> {
    pub ctx: GuiContext<'a>,
    pub state: &'a mut MidiPanelState,
    pub slots: &'a [SlotStatus],
    pub master_strobe_fader_channel_mapped: bool,
}

impl MidiPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        ui.heading("MIDI Devices");
        ui.separator();

        ui.horizontal(|ui| {
            if ui
                .button("\u{1f504}")
                .on_hover_text("Refresh port list")
                .clicked()
            {
                self.state.refresh_ports();
            }
        });
        ui.add_space(4.0);

        // Copy the reference out of self so the loop doesn't conflict with
        // &mut self method calls.
        let slots = self.slots;

        if slots.is_empty() {
            ui.label("No MIDI slots configured.");
            return;
        }

        egui::Grid::new("midi_slots_grid")
            .num_columns(6)
            .spacing([12.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Slot");
                ui.strong("Model");
                ui.strong("Input");
                ui.strong("Output");
                ui.label(""); // Auto button column
                ui.label(""); // Clear button column
                ui.end_row();

                for slot in slots {
                    ui.label(&slot.name);
                    ui.label(&slot.model);

                    self.port_combo(ui, &slot.name, &slot.input, DeviceKind::Input);
                    self.port_combo(ui, &slot.name, &slot.output, DeviceKind::Output);

                    if ui.button("Auto").clicked() {
                        self.auto_configure_slot(&slot.model, &slot.name);
                    }

                    if ui.button("Clear").clicked() {
                        let _ = self.ctx.send_command(MetaCommand::ClearMidiDevice {
                            slot_name: slot.name.clone(),
                        });
                    }

                    ui.end_row();
                }
            });

        ui.separator();
        let mut strobe_enabled = self.master_strobe_fader_channel_mapped;
        if ui
            .checkbox(
                &mut strobe_enabled,
                "Use last wing fader as master strobe control",
            )
            .changed()
        {
            let _ = self
                .ctx
                .send_command(MetaCommand::SetMasterStrobeChannel(strobe_enabled));
        }
    }

    /// Render a combo box for selecting a MIDI port.
    fn port_combo(
        &mut self,
        ui: &mut egui::Ui,
        slot_name: &str,
        current: &PortStatus,
        kind: DeviceKind,
    ) {
        let kind_label = match kind {
            DeviceKind::Input => "input",
            DeviceKind::Output => "output",
        };
        let combo_id = format!("{slot_name}_{kind_label}");

        let (selected_text, text_color) = match current {
            PortStatus::Unassigned => ("Unassigned".to_string(), STATUS_COLORS.inactive),
            PortStatus::Connected { name, .. } => (name.clone(), STATUS_COLORS.active),
            PortStatus::Disconnected { name, .. } => {
                (format!("{name} (disconnected)"), STATUS_COLORS.error)
            }
        };

        let current_id = match current {
            PortStatus::Connected { id, .. } | PortStatus::Disconnected { id, .. } => Some(id),
            PortStatus::Unassigned => None,
        };

        let ports = self.state.ports_for_kind(kind);
        let ctx = &mut self.ctx;

        egui::ComboBox::from_id_salt(&combo_id)
            .selected_text(egui::RichText::new(&selected_text).color(text_color))
            .show_ui(ui, |ui| {
                if current_id.is_some() && ui.selectable_label(false, "Unassigned").clicked() {
                    let _ = ctx.send_command(MetaCommand::ClearMidiDevice {
                        slot_name: slot_name.to_string(),
                    });
                }

                for port in ports {
                    let is_selected = current_id == Some(&port.id);
                    if ui.selectable_label(is_selected, &port.name).clicked() && !is_selected {
                        let _ = ctx.send_command(MetaCommand::ConnectMidiPort {
                            slot_name: slot_name.to_string(),
                            device_id: port.id.clone(),
                            kind,
                        });
                    }
                }
            });
    }

    /// Try to auto-configure a slot by matching its model name against available ports.
    fn auto_configure_slot(&mut self, model: &str, slot_name: &str) {
        // Refresh so we don't match against stale ports from unplugged devices.
        self.state.refresh_ports();

        let input_id = self
            .state
            .input_ports
            .iter()
            .find(|p| p.name == model)
            .map(|p| p.id.clone());
        let output_id = self
            .state
            .output_ports
            .iter()
            .find(|p| p.name == model)
            .map(|p| p.id.clone());

        if input_id.is_none() && output_id.is_none() {
            self.ctx
                .report_error(format_args!("No matching MIDI port found for \"{model}\""));
            return;
        }

        if let Some(id) = input_id {
            let _ = self.ctx.send_command(MetaCommand::ConnectMidiPort {
                slot_name: slot_name.to_string(),
                device_id: id,
                kind: DeviceKind::Input,
            });
        }
        if let Some(id) = output_id {
            let _ = self.ctx.send_command(MetaCommand::ConnectMidiPort {
                slot_name: slot_name.to_string(),
                device_id: id,
                kind: DeviceKind::Output,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::mock::auto_respond_client;
    use crate::ui_util::MessageModal;
    use egui_kittest::Harness;
    use midi_harness::DeviceId;

    fn test_slots() -> Vec<SlotStatus> {
        vec![
            SlotStatus {
                name: "Submaster Wing 1".to_string(),
                model: "Launch Control XL".to_string(),
                input: PortStatus::Connected {
                    id: DeviceId("lxcl-in".into()),
                    name: "LXCL Input".to_string(),
                },
                output: PortStatus::Disconnected {
                    id: DeviceId("lxcl-out".into()),
                    name: "LXCL Output".to_string(),
                },
            },
            SlotStatus {
                name: "Clock Wing".to_string(),
                model: "CMD MM-1".to_string(),
                input: PortStatus::Unassigned,
                output: PortStatus::Unassigned,
            },
            SlotStatus {
                name: "Fader Wing".to_string(),
                model: "nanoKONTROL2".to_string(),
                input: PortStatus::Connected {
                    id: DeviceId("nano-in".into()),
                    name: "nanoKONTROL2 MIDI In".to_string(),
                },
                output: PortStatus::Connected {
                    id: DeviceId("nano-out".into()),
                    name: "nanoKONTROL2 MIDI Out".to_string(),
                },
            },
        ]
    }

    #[test]
    fn render_empty_slots() {
        let client = auto_respond_client();
        let mut modal = MessageModal::default();
        let slots: Vec<SlotStatus> = vec![];
        let mut harness = Harness::new_ui(|ui| {
            MidiPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut MidiPanelState {
                    input_ports: vec![],
                    output_ports: vec![],
                },
                slots: &slots,
                master_strobe_fader_channel_mapped: false,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("midi_panel_empty");
    }

    #[test]
    fn render_populated_slots() {
        let client = auto_respond_client();
        let mut modal = MessageModal::default();
        let slots = test_slots();
        let mut harness = Harness::new_ui(|ui| {
            MidiPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut MidiPanelState {
                    input_ports: vec![],
                    output_ports: vec![],
                },
                slots: &slots,
                master_strobe_fader_channel_mapped: false,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("midi_panel_populated");
    }
}
