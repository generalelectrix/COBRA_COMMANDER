//! egui console application for live show management.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use eframe::egui;

use super::command::ConsoleCommand;
use super::state::{ConsoleState, FixtureTypeMeta};
use crate::fixture::patch::PatchOption;

/// The egui console application.
pub struct ConsoleApp {
    /// Shared state read from the Show.
    state: Arc<Mutex<ConsoleState>>,
    /// Send commands back to the Show.
    commands: std::sync::mpsc::Sender<ConsoleCommand>,
    /// Close confirmation handler.
    close_handler: CloseHandler,
    /// Text buffer for the OSC client address input.
    osc_addr_input: String,
    /// Whether the patch editor panel is open.
    show_editor: bool,
    /// The draft patch being edited.
    draft: DraftPatch,
    /// Local error message for the console UI.
    local_error: Option<String>,
}

impl ConsoleApp {
    pub fn new(
        state: Arc<Mutex<ConsoleState>>,
        commands: std::sync::mpsc::Sender<ConsoleCommand>,
    ) -> Self {
        Self {
            state,
            commands,
            close_handler: Default::default(),
            osc_addr_input: String::new(),
            show_editor: false,
            draft: DraftPatch::default(),
            local_error: None,
        }
    }

    /// Initialize draft from the current live state.
    fn init_draft_from_live(&mut self, state: &ConsoleState) {
        self.draft.groups = state
            .groups
            .iter()
            .map(|g| {
                let fixture_type_idx = state
                    .fixture_types
                    .iter()
                    .position(|ft| ft.name == g.fixture_type || g.fixture_type.contains(&ft.name))
                    .unwrap_or(0);
                DraftGroup {
                    fixture_type_idx,
                    group_name: if g.key != g.fixture_type {
                        g.key.clone()
                    } else {
                        String::new()
                    },
                    channel: g.channel,
                    color_organ: g.color_organ,
                    group_options: g.options.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    patches: g
                        .patches
                        .iter()
                        .map(|p| DraftPatchEntry {
                            addr: p.addr.map(|a| a.to_string()).unwrap_or_default(),
                            universe: p.universe.to_string(),
                            mirror: p.mirror,
                            patch_options: p
                                .options
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect(),
                        })
                        .collect(),
                }
            })
            .collect();
    }
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler
            .update("Are you sure you want to close the console?", ctx);

        // Request periodic repaints to reflect Show state changes.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        let state = self.state.lock().unwrap().clone();

        // --- Left side panel: OSC and MIDI ---
        egui::SidePanel::left("control_panel")
            .min_width(200.0)
            .show(ctx, |ui| {
                ui.heading("OSC Clients");
                ui.separator();

                if state.osc_clients.is_empty() {
                    ui.label("No OSC clients connected.");
                } else {
                    let mut to_remove = None;
                    for (i, client) in state.osc_clients.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(client.to_string());
                            if ui.small_button("✕").clicked() {
                                to_remove = Some(*client);
                            }
                        });
                        if i < state.osc_clients.len() - 1 {
                            ui.separator();
                        }
                    }
                    if let Some(addr) = to_remove {
                        let _ = self.commands.send(ConsoleCommand::RemoveOscClient(addr));
                    }
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.osc_addr_input);
                    if ui.button("Add").clicked() {
                        match self.osc_addr_input.parse::<SocketAddr>() {
                            Ok(addr) => {
                                let _ = self.commands.send(ConsoleCommand::AddOscClient(addr));
                                self.osc_addr_input.clear();
                                self.local_error = None;
                            }
                            Err(_) => {
                                self.local_error =
                                    Some(format!("Invalid address: '{}'", self.osc_addr_input));
                            }
                        }
                    }
                });

                ui.add_space(16.0);
                ui.heading("MIDI Devices");
                ui.separator();

                if state.midi_inputs.is_empty() {
                    ui.label("No MIDI devices.");
                } else {
                    for name in &state.midi_inputs {
                        ui.label(name);
                    }
                }
                if ui.button("Rescan MIDI").clicked() {
                    let _ = self.commands.send(ConsoleCommand::RescanMidi);
                }
            });

        // --- Right side panel: Patch editor ---
        egui::SidePanel::right("patch_editor")
            .min_width(300.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Patch Editor");
                    if ui.button(if self.show_editor { "Hide" } else { "Show" }).clicked() {
                        self.show_editor = !self.show_editor;
                    }
                });
                ui.separator();

                if !self.show_editor {
                    ui.label("Click Show to open the patch editor.");
                    return;
                }

                ui.horizontal(|ui| {
                    if ui.button("Load from live").clicked() {
                        self.init_draft_from_live(&state);
                    }
                    if ui.button("Clear").clicked() {
                        self.draft = DraftPatch::default();
                    }
                    if ui.button("Apply").clicked() {
                        match self.draft.try_build_configs(&state.fixture_types) {
                            Ok(configs) => {
                                let _ = self.commands.send(ConsoleCommand::Repatch(configs));
                                self.local_error = None;
                            }
                            Err(e) => {
                                self.local_error = Some(e);
                            }
                        }
                    }
                });

                ui.separator();

                if ui.button("+ Add Group").clicked() {
                    self.draft.groups.push(DraftGroup::default());
                }

                ui.add_space(8.0);

                let fixture_type_names: Vec<&str> =
                    state.fixture_types.iter().map(|ft| ft.name.as_str()).collect();

                egui::ScrollArea::vertical()
                    .id_salt("draft_editor_scroll")
                    .show(ui, |ui| {
                        let mut remove_group = None;
                        for (gi, group) in self.draft.groups.iter_mut().enumerate() {
                            let group_label = if group.group_name.is_empty() {
                                fixture_type_names
                                    .get(group.fixture_type_idx)
                                    .copied()
                                    .unwrap_or("?")
                                    .to_string()
                            } else {
                                group.group_name.clone()
                            };

                            egui::CollapsingHeader::new(format!("Group {}: {}", gi, group_label))
                                .default_open(true)
                                .id_salt(format!("draft_group_{gi}"))
                                .show(ui, |ui| {
                                    // Fixture type dropdown
                                    ui.horizontal(|ui| {
                                        ui.label("Fixture:");
                                        egui::ComboBox::from_id_salt(format!("fixture_type_{gi}"))
                                            .selected_text(
                                                fixture_type_names
                                                    .get(group.fixture_type_idx)
                                                    .copied()
                                                    .unwrap_or("Select..."),
                                            )
                                            .show_ui(ui, |ui| {
                                                for (idx, name) in
                                                    fixture_type_names.iter().enumerate()
                                                {
                                                    ui.selectable_value(
                                                        &mut group.fixture_type_idx,
                                                        idx,
                                                        *name,
                                                    );
                                                }
                                            });
                                    });

                                    // Group name
                                    ui.horizontal(|ui| {
                                        ui.label("Group name:");
                                        ui.text_edit_singleline(&mut group.group_name);
                                    });

                                    // Toggles
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut group.channel, "Channel");
                                        ui.checkbox(&mut group.color_organ, "Color organ");
                                    });

                                    // Group-level options
                                    if let Some(ft) =
                                        state.fixture_types.get(group.fixture_type_idx)
                                        && !ft.group_options.is_empty()
                                    {
                                        ui.label("Group options:");
                                        render_options(
                                            ui,
                                            &ft.group_options,
                                            &mut group.group_options,
                                            &format!("gopt_{gi}"),
                                        );
                                    }

                                    ui.add_space(4.0);

                                    // Patches
                                    if ui.small_button("+ Add Patch").clicked() {
                                        group.patches.push(DraftPatchEntry::default());
                                    }

                                    let mut remove_patch = None;
                                    for (pi, patch) in group.patches.iter_mut().enumerate() {
                                        ui.indent(format!("draft_patch_{gi}_{pi}"), |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label(format!("Patch {}:", pi));
                                                if ui.small_button("✕").clicked() {
                                                    remove_patch = Some(pi);
                                                }
                                            });
                                            ui.horizontal(|ui| {
                                                ui.label("Addr:");
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut patch.addr)
                                                        .desired_width(60.0)
                                                        .hint_text("e.g. 1"),
                                                );
                                                ui.label("Univ:");
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut patch.universe,
                                                    )
                                                    .desired_width(30.0)
                                                    .hint_text("0"),
                                                );
                                                ui.checkbox(&mut patch.mirror, "Mirror");
                                            });

                                            // Patch-level options
                                            if let Some(ft) =
                                                state.fixture_types.get(group.fixture_type_idx)
                                                && !ft.patch_options.is_empty()
                                            {
                                                render_options(
                                                    ui,
                                                    &ft.patch_options,
                                                    &mut patch.patch_options,
                                                    &format!("popt_{gi}_{pi}"),
                                                );
                                            }
                                        });
                                    }
                                    if let Some(pi) = remove_patch {
                                        group.patches.remove(pi);
                                    }

                                    ui.add_space(4.0);
                                    if ui.small_button("Remove group").clicked() {
                                        remove_group = Some(gi);
                                    }
                                });
                        }
                        if let Some(gi) = remove_group {
                            self.draft.groups.remove(gi);
                        }
                    });
            });

        // --- Central panel: Current patch ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Current Patch");

            // Show errors if any.
            if let Some(err) = &state.last_error {
                ui.colored_label(egui::Color32::RED, format!("Show error: {err}"));
            }
            if let Some(err) = &self.local_error {
                ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
            }
            if state.last_error.is_some() || self.local_error.is_some() {
                ui.separator();
            }

            if state.groups.is_empty() {
                ui.label("No fixture groups in patch.");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("live_patch_scroll")
                    .show(ui, |ui| {
                    for group in &state.groups {
                        egui::CollapsingHeader::new(&group.key)
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Type:");
                                    ui.strong(&group.fixture_type);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Channel:");
                                    ui.label(if group.channel { "Yes" } else { "No" });
                                });
                                if group.color_organ {
                                    ui.label("Color organ: Yes");
                                }
                                if !group.options.is_empty() {
                                    ui.label("Options:");
                                    for (key, val) in &group.options {
                                        ui.horizontal(|ui| {
                                            ui.label(format!("  {key}:"));
                                            ui.label(val);
                                        });
                                    }
                                }

                                ui.add_space(4.0);
                                ui.label(format!("{} patch(es)", group.patches.len()));
                                for (i, patch) in group.patches.iter().enumerate() {
                                    ui.indent(format!("patch_{}", i), |ui| {
                                        ui.horizontal(|ui| {
                                            if let Some(addr) = patch.addr {
                                                ui.label(format!(
                                                    "Addr: {} (Univ {})",
                                                    addr, patch.universe
                                                ));
                                            } else {
                                                ui.label("Non-DMX fixture");
                                            }
                                            if patch.mirror {
                                                ui.label("[mirrored]");
                                            }
                                            ui.label(format!("{} ch", patch.channel_count));
                                        });
                                    });
                                }
                            });
                    }
                });
            }
        });
    }
}

// --- Draft patch types ---

/// A draft patch being edited in the console.
#[derive(Default)]
struct DraftPatch {
    groups: Vec<DraftGroup>,
}

/// A draft fixture group.
struct DraftGroup {
    /// Index into ConsoleState::fixture_types.
    fixture_type_idx: usize,
    /// Optional group name override.
    group_name: String,
    /// Assign to a channel.
    channel: bool,
    /// Use a color organ.
    color_organ: bool,
    /// Group-level options as editable key/value strings.
    group_options: Vec<(String, String)>,
    /// Individual patches in this group.
    patches: Vec<DraftPatchEntry>,
}

impl Default for DraftGroup {
    fn default() -> Self {
        Self {
            fixture_type_idx: 0,
            group_name: String::new(),
            channel: true,
            color_organ: false,
            group_options: vec![],
            patches: vec![DraftPatchEntry::default()],
        }
    }
}

/// A draft patch entry within a group.
#[derive(Default)]
struct DraftPatchEntry {
    /// DMX address as a string (supports "1" or "1:3" for start:count).
    addr: String,
    /// Universe index as a string.
    universe: String,
    /// Mirror this patch.
    mirror: bool,
    /// Patch-level options as editable key/value strings.
    patch_options: Vec<(String, String)>,
}

impl DraftPatch {
    /// Attempt to build FixtureGroupConfig values from the draft.
    fn try_build_configs(
        &self,
        fixture_types: &[FixtureTypeMeta],
    ) -> Result<Vec<crate::config::FixtureGroupConfig>, String> {
        if self.groups.is_empty() {
            return Err("No groups in draft.".into());
        }

        // Validate that all groups have at least one patch.
        for (i, group) in self.groups.iter().enumerate() {
            if group.patches.is_empty() {
                let name = fixture_types
                    .get(group.fixture_type_idx)
                    .map(|ft| ft.name.as_str())
                    .unwrap_or("Unknown");
                return Err(format!("Group {i} ({name}) has no patches."));
            }
        }

        // Build a YAML value from the draft and parse it via serde.
        // This reuses the existing parsing infrastructure rather than
        // manually constructing FixtureGroupConfig values.
        let mut yaml_groups = Vec::new();

        for group in &self.groups {
            let fixture_name = fixture_types
                .get(group.fixture_type_idx)
                .map(|ft| ft.name.as_str())
                .unwrap_or("Unknown");

            let mut mapping = serde_yaml::Mapping::new();
            mapping.insert(
                serde_yaml::Value::String("fixture".into()),
                serde_yaml::Value::String(fixture_name.into()),
            );

            if !group.group_name.is_empty() {
                mapping.insert(
                    serde_yaml::Value::String("group".into()),
                    serde_yaml::Value::String(group.group_name.clone()),
                );
            }

            mapping.insert(
                serde_yaml::Value::String("channel".into()),
                serde_yaml::Value::Bool(group.channel),
            );

            if group.color_organ {
                mapping.insert(
                    serde_yaml::Value::String("color_organ".into()),
                    serde_yaml::Value::Bool(true),
                );
            }

            // Group-level options are flattened into the top level.
            for (key, val) in &group.group_options {
                if !val.is_empty() {
                    mapping.insert(
                        serde_yaml::Value::String(key.clone()),
                        parse_yaml_value(val),
                    );
                }
            }

            // Build patches array.
            let patches: Vec<serde_yaml::Value> = group
                .patches
                .iter()
                .map(|p| {
                    let mut pm = serde_yaml::Mapping::new();

                    if !p.addr.is_empty() {
                        // Support "start:count" syntax.
                        if let Some((start, count)) = p.addr.split_once(':') {
                            let mut addr_map = serde_yaml::Mapping::new();
                            addr_map.insert(
                                serde_yaml::Value::String("start".into()),
                                parse_yaml_value(start.trim()),
                            );
                            addr_map.insert(
                                serde_yaml::Value::String("count".into()),
                                parse_yaml_value(count.trim()),
                            );
                            pm.insert(
                                serde_yaml::Value::String("addr".into()),
                                serde_yaml::Value::Mapping(addr_map),
                            );
                        } else {
                            pm.insert(
                                serde_yaml::Value::String("addr".into()),
                                parse_yaml_value(&p.addr),
                            );
                        }
                    }

                    let univ: usize = p.universe.parse().unwrap_or(0);
                    if univ != 0 {
                        pm.insert(
                            serde_yaml::Value::String("universe".into()),
                            serde_yaml::Value::Number(serde_yaml::Number::from(univ)),
                        );
                    }

                    if p.mirror {
                        pm.insert(
                            serde_yaml::Value::String("mirror".into()),
                            serde_yaml::Value::Bool(true),
                        );
                    }

                    // Patch-level options are flattened.
                    for (key, val) in &p.patch_options {
                        if !val.is_empty() {
                            pm.insert(
                                serde_yaml::Value::String(key.clone()),
                                parse_yaml_value(val),
                            );
                        }
                    }

                    serde_yaml::Value::Mapping(pm)
                })
                .collect();

            mapping.insert(
                serde_yaml::Value::String("patches".into()),
                serde_yaml::Value::Sequence(patches),
            );

            yaml_groups.push(serde_yaml::Value::Mapping(mapping));
        }

        let yaml_value = serde_yaml::Value::Sequence(yaml_groups);
        serde_yaml::from_value(yaml_value)
            .map_err(|e| format!("Failed to build patch config: {e}"))
    }
}

/// Parse a string value into a serde_yaml Value, trying integer then bool then string.
fn parse_yaml_value(s: &str) -> serde_yaml::Value {
    if let Ok(n) = s.parse::<i64>() {
        return serde_yaml::Value::Number(serde_yaml::Number::from(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        return serde_yaml::Value::Number(serde_yaml::Number::from(n));
    }
    match s {
        "true" => serde_yaml::Value::Bool(true),
        "false" => serde_yaml::Value::Bool(false),
        _ => serde_yaml::Value::String(s.to_string()),
    }
}

/// Render option widgets for a list of PatchOption definitions.
fn render_options(
    ui: &mut egui::Ui,
    option_defs: &[(String, PatchOption)],
    values: &mut Vec<(String, String)>,
    id_prefix: &str,
) {
    // Ensure we have entries for all defined options.
    for (key, _) in option_defs {
        if !values.iter().any(|(k, _)| k == key) {
            let entry: (String, String) = (key.clone(), String::new());
            values.push(entry);
        }
    }

    for (key, opt) in option_defs {
        let val = values
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap();

        ui.horizontal(|ui| {
            ui.label(format!("  {key}:"));
            match opt {
                PatchOption::Bool => {
                    let mut checked = val == "true";
                    if ui.checkbox(&mut checked, "").clicked() {
                        *val = checked.to_string();
                    }
                }
                PatchOption::Select(variants) => {
                    let selected = if val.is_empty() {
                        "Select...".to_string()
                    } else {
                        val.clone()
                    };
                    egui::ComboBox::from_id_salt(format!("{id_prefix}_{key}"))
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for variant in variants {
                                ui.selectable_value(val, variant.to_string(), variant);
                            }
                        });
                }
                PatchOption::Int | PatchOption::SocketAddr | PatchOption::Url => {
                    let hint = match opt {
                        PatchOption::Int => "integer",
                        PatchOption::SocketAddr => "addr:port",
                        PatchOption::Url => "http://...",
                        _ => "",
                    };
                    ui.add(
                        egui::TextEdit::singleline(val)
                            .desired_width(120.0)
                            .hint_text(hint),
                    );
                }
            }
        });
    }
}

/// Handle for confirming window close.
#[derive(Default)]
struct CloseHandler {
    show_confirmation_dialog: bool,
    allowed_to_close: bool,
}

impl CloseHandler {
    fn update(&mut self, quit_prompt: &str, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allowed_to_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_confirmation_dialog = true;
        }

        if self.show_confirmation_dialog {
            egui::Window::new(quit_prompt)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("No").clicked() {
                            self.show_confirmation_dialog = false;
                            self.allowed_to_close = false;
                        }

                        if ui.button("Yes").clicked() {
                            self.show_confirmation_dialog = false;
                            self.allowed_to_close = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
    }
}
