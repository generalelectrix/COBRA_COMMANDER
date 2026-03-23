use std::collections::BTreeMap;

use eframe::egui;

use crate::config::{DmxAddrConfig, FixtureGroupConfig, FixtureGroupKey, Options, PatchBlock};
use crate::control::MetaCommand;
use crate::dmx::DmxAddr;
use crate::fixture::patch::{PatchOption, Patcher};
use crate::gui_state::PatchSnapshot;
use crate::ui_util::{GuiContext, StatusColors};

// ---------------------------------------------------------------------------
// Working copy types
// ---------------------------------------------------------------------------

struct WorkingGroup {
    config: FixtureGroupConfig,
    /// Channel count per patch block, resolved at creation via
    /// patcher.create_patch(). One entry per PatchBlock in config.patches.
    channel_counts: Vec<usize>,
}

struct PatchWorkingCopy {
    groups: Vec<WorkingGroup>,
}

impl PatchWorkingCopy {
    fn from_snapshot(snapshot: &PatchSnapshot, patchers: &[Patcher]) -> Self {
        let groups = snapshot
            .groups
            .iter()
            .map(|group_cfg| Self::resolve_group(group_cfg, patchers))
            .collect();
        Self { groups }
    }

    fn resolve_group(group_cfg: &FixtureGroupConfig, patchers: &[Patcher]) -> WorkingGroup {
        let patcher = patchers.iter().find(|p| p.name.0 == group_cfg.fixture);
        let channel_counts = group_cfg
            .patches
            .iter()
            .map(|block| resolve_channel_count(patcher, group_cfg, block))
            .collect();
        WorkingGroup {
            config: group_cfg.clone(),
            channel_counts,
        }
    }

    fn configs(&self) -> Vec<FixtureGroupConfig> {
        self.groups.iter().map(|g| g.config.clone()).collect()
    }
}

fn resolve_channel_count(
    patcher: Option<&Patcher>,
    group_cfg: &FixtureGroupConfig,
    block: &PatchBlock,
) -> usize {
    patcher
        .and_then(|p| {
            (p.create_patch)(group_cfg.options.clone(), block.options.clone())
                .ok()
                .map(|cfg| cfg.channel_count)
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Add-group form state
// ---------------------------------------------------------------------------

struct AddGroupForm {
    fixture_type_idx: usize,
    group_name: String,
    channel: bool,
    color_organ: bool,
    group_options: Vec<(String, String)>,
    first_addr: String,
    first_universe: String,
}

impl AddGroupForm {
    fn new() -> Self {
        Self {
            fixture_type_idx: 0,
            group_name: String::new(),
            channel: true,
            color_organ: false,
            group_options: Vec::new(),
            first_addr: "1".to_string(),
            first_universe: "0".to_string(),
        }
    }

    fn sync_options(&mut self, patchers: &[Patcher]) {
        if let Some(patcher) = patchers.get(self.fixture_type_idx) {
            let menu = (patcher.group_options)();
            self.group_options = menu
                .iter()
                .map(|(key, opt)| (key.clone(), default_for_option(opt)))
                .collect();
        }
    }
}

fn default_for_option(opt: &PatchOption) -> String {
    match opt {
        PatchOption::Bool => "false".to_string(),
        PatchOption::Int => "0".to_string(),
        PatchOption::Select(choices) => choices.first().cloned().unwrap_or_default(),
        PatchOption::Url => String::new(),
        PatchOption::SocketAddr => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Add-fixture inline state
// ---------------------------------------------------------------------------

struct AddFixtureForm {
    addr: String,
    universe: String,
    count: String,
    mirror: bool,
    patch_options: Vec<(String, String)>,
}

impl AddFixtureForm {
    fn new_for_group(group: &WorkingGroup, patchers: &[Patcher]) -> Self {
        // Pre-populate address with next available.
        let next_addr = group
            .config
            .patches
            .last()
            .and_then(|b| {
                let (start, count) = b.start_count();
                let ch = group.channel_counts.last().copied().unwrap_or(0);
                start.map(|a| a + ch * count)
            })
            .map(|a| format!("{a}"))
            .unwrap_or_else(|| "1".to_string());

        let patcher = patchers.iter().find(|p| p.name.0 == group.config.fixture);
        let patch_options = patcher
            .map(|p| {
                (p.patch_options)()
                    .iter()
                    .map(|(key, opt)| (key.clone(), default_for_option(opt)))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            addr: next_addr,
            universe: "0".to_string(),
            count: "1".to_string(),
            mirror: false,
            patch_options,
        }
    }
}

// ---------------------------------------------------------------------------
// Collision detection
// ---------------------------------------------------------------------------

/// An entry in the DMX address map.
struct AddrMapEntry {
    group_name: String,
    #[expect(unused)]
    group_idx: usize,
}

/// Build a map of (universe, dmx_addr_1indexed) -> group info from the working copy.
fn build_address_map(wc: &PatchWorkingCopy) -> BTreeMap<(usize, usize), Vec<AddrMapEntry>> {
    let mut map: BTreeMap<(usize, usize), Vec<AddrMapEntry>> = BTreeMap::new();
    for (gi, group) in wc.groups.iter().enumerate() {
        let name = group.config.key().to_string();
        for (pi, block) in group.config.patches.iter().enumerate() {
            let (start, count) = block.start_count();
            let Some(start_addr) = start else { continue };
            let ch_count = group.channel_counts.get(pi).copied().unwrap_or(0);
            if ch_count == 0 {
                continue;
            }
            let mut addr = start_addr;
            for _ in 0..count {
                let base = addr.dmx_index() + 1; // back to 1-indexed
                for ch in 0..ch_count {
                    map.entry((block.universe, base + ch))
                        .or_default()
                        .push(AddrMapEntry {
                            group_name: name.clone(),
                            group_idx: gi,
                        });
                }
                addr = addr + ch_count;
            }
        }
    }
    map
}

/// Check if a specific address has a collision in the address map.
fn collision_at(
    addr_map: &BTreeMap<(usize, usize), Vec<AddrMapEntry>>,
    universe: usize,
    addr: usize,
) -> Option<String> {
    addr_map.get(&(universe, addr)).and_then(|entries| {
        if entries.len() > 1 {
            let names: Vec<_> = entries.iter().map(|e| e.group_name.as_str()).collect();
            Some(names.join(", "))
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

fn validate_option(opt: &PatchOption, value: &str) -> Result<(), String> {
    match opt {
        PatchOption::Bool | PatchOption::Select(_) => Ok(()),
        PatchOption::Int => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "must be a number".to_string()),
        PatchOption::Url => url::Url::parse(value)
            .map(|_| ())
            .map_err(|e| format!("invalid URL: {e}")),
        PatchOption::SocketAddr => value
            .parse::<std::net::SocketAddr>()
            .map(|_| ())
            .map_err(|_| "invalid address (expected host:port)".to_string()),
    }
}

fn build_options_from_form(entries: &[(String, String)]) -> Options {
    Options::from_entries(entries.iter().map(|(k, v)| {
        let yaml_val = if v == "true" {
            serde_yaml::Value::Bool(true)
        } else if v == "false" {
            serde_yaml::Value::Bool(false)
        } else if let Ok(n) = v.parse::<i64>() {
            serde_yaml::Value::Number(n.into())
        } else {
            serde_yaml::Value::String(v.clone())
        };
        (k.clone(), yaml_val)
    }))
}

// ---------------------------------------------------------------------------
// Panel state
// ---------------------------------------------------------------------------

enum PanelMode {
    View,
    AddGroup(AddGroupForm),
    AddFixture(AddFixtureForm),
    ConfirmDeleteGroup(usize),
}

pub struct PatchPanelState {
    working_copy: Option<PatchWorkingCopy>,
    selected_group: Option<usize>,
    show_address_map: bool,
    mode: PanelMode,
}

impl PatchPanelState {
    pub fn new() -> Self {
        Self {
            working_copy: None,
            selected_group: None,
            show_address_map: false,
            mode: PanelMode::View,
        }
    }
}

// ---------------------------------------------------------------------------
// Panel
// ---------------------------------------------------------------------------

pub(crate) struct PatchPanel<'a> {
    pub ctx: GuiContext<'a>,
    pub state: &'a mut PatchPanelState,
    pub snapshot: &'a PatchSnapshot,
    pub patchers: &'a [Patcher],
    pub status_colors: &'a StatusColors,
}

impl PatchPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        // Initialize working copy from snapshot if not yet created.
        if self.state.working_copy.is_none() {
            self.state.working_copy = Some(PatchWorkingCopy::from_snapshot(
                self.snapshot,
                self.patchers,
            ));
        }

        // === Header with Add button and DMX Map toggle ===
        ui.horizontal(|ui| {
            ui.heading("Patch");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Add").clicked() {
                    let mut form = AddGroupForm::new();
                    form.sync_options(self.patchers);
                    self.state.mode = PanelMode::AddGroup(form);
                }
                ui.toggle_value(&mut self.state.show_address_map, "DMX Map");
            });
        });
        ui.separator();

        // === DMX Address Map side panel ===
        if self.state.show_address_map {
            let wc = self.state.working_copy.as_ref().unwrap();
            let addr_map = build_address_map(wc);
            egui::SidePanel::right("dmx_address_map")
                .default_width(200.0)
                .show_inside(ui, |ui| {
                    self.render_address_map(ui, wc, &addr_map);
                });
        }

        // === Main content ===
        match &self.state.mode {
            PanelMode::AddGroup(_) => {
                self.render_add_group_form(ui);
            }
            _ => {
                self.render_main_view(ui);
            }
        }
    }

    fn render_main_view(&mut self, ui: &mut egui::Ui) {
        let wc = self.state.working_copy.as_ref().unwrap();

        if wc.groups.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("No fixtures patched.");
                ui.label("Click [+ Add] to add a group.");
            });
            return;
        }

        // Clamp selected_group.
        let num_groups = wc.groups.len();
        if let Some(sel) = self.state.selected_group
            && sel >= num_groups {
                self.state.selected_group = Some(num_groups - 1);
            }

        // === Group list with channel reorder buttons (WP5) ===
        let mut swap: Option<(usize, usize)> = None;

        egui::ScrollArea::vertical()
            .max_height(120.0)
            .id_salt("patch_group_list")
            .show(ui, |ui| {
                let wc = self.state.working_copy.as_ref().unwrap();
                let n = wc.groups.len();
                for i in 0..n {
                    let group = &wc.groups[i];
                    let has_channel = group.config.channel;

                    ui.horizontal(|ui| {
                        // Up/down buttons for channel ordering
                        if has_channel {
                            if ui
                                .add_enabled(i > 0, egui::Button::new("Up").small())
                                .clicked()
                            {
                                swap = Some((i, i - 1));
                            }
                            if ui
                                .add_enabled(i < n - 1, egui::Button::new("Dn").small())
                                .clicked()
                            {
                                swap = Some((i, i + 1));
                            }
                        } else {
                            // Spacer to align with groups that have buttons.
                            ui.add_space(36.0);
                        }

                        let key = group.config.key();
                        let fixture_type = &group.config.fixture;
                        let fix_count = group.config.patches.len();
                        let label = format!(
                            "{key} ({fixture_type}){}    {fix_count} fix",
                            if has_channel { "  ch" } else { "" },
                        );

                        let is_selected = self.state.selected_group == Some(i);
                        if ui.selectable_label(is_selected, &label).clicked() {
                            self.state.selected_group = Some(i);
                            // Cancel any in-progress add-fixture when switching groups.
                            if matches!(self.state.mode, PanelMode::AddFixture(_)) {
                                self.state.mode = PanelMode::View;
                            }
                        }
                    });
                }
            });

        // Apply swap if requested.
        if let Some((a, b)) = swap {
            let wc = self.state.working_copy.as_mut().unwrap();
            wc.groups.swap(a, b);
            // Follow selection.
            if self.state.selected_group == Some(a) {
                self.state.selected_group = Some(b);
            } else if self.state.selected_group == Some(b) {
                self.state.selected_group = Some(a);
            }
        }

        ui.separator();

        // === Apply / Revert buttons ===
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                let wc = self.state.working_copy.as_ref().unwrap();
                let configs = wc.configs();
                if self.ctx.send_command(MetaCommand::Repatch(configs)).is_ok() {
                    self.state.working_copy = None; // Re-clone from snapshot next frame.
                    self.state.mode = PanelMode::View;
                }
            }
            if ui.button("Revert").clicked() {
                self.state.working_copy = None;
                self.state.selected_group = None;
                self.state.mode = PanelMode::View;
            }
        });

        ui.separator();

        // === Detail view ===
        if let Some(sel) = self.state.selected_group {
            let wc = self.state.working_copy.as_ref().unwrap();
            if sel < wc.groups.len() {
                self.render_detail(ui, sel);
            }
        }
    }

    fn render_detail(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        // === Header with delete ===
        let (key, fixture_type) = {
            let wc = self.state.working_copy.as_ref().unwrap();
            let cfg = &wc.groups[group_idx].config;
            (cfg.key().to_string(), cfg.fixture.clone())
        };

        // Delete confirmation flow.
        if let PanelMode::ConfirmDeleteGroup(idx) = self.state.mode
            && idx == group_idx {
                let wc = self.state.working_copy.as_ref().unwrap();
                let fix_count = wc.groups[group_idx].config.patches.len();
                ui.colored_label(
                    self.status_colors.error,
                    format!("Really delete {key} ({fix_count} fixtures)?"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.state.mode = PanelMode::View;
                    }
                    if ui.button("Delete").clicked() {
                        let wc = self.state.working_copy.as_mut().unwrap();
                        wc.groups.remove(group_idx);
                        self.state.selected_group = if wc.groups.is_empty() {
                            None
                        } else {
                            Some(group_idx.min(wc.groups.len() - 1))
                        };
                        self.state.mode = PanelMode::View;
                    }
                });
                return;
            }

        ui.horizontal(|ui| {
            ui.heading(&key);
            ui.label(format!("({fixture_type})"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Del").clicked() {
                    self.state.mode = PanelMode::ConfirmDeleteGroup(group_idx);
                }
            });
        });
        ui.separator();

        // === Channel / Color Organ (editable) ===
        {
            let wc = self.state.working_copy.as_mut().unwrap();
            let cfg = &mut wc.groups[group_idx].config;
            ui.horizontal(|ui| {
                ui.checkbox(&mut cfg.channel, "Channel");
                ui.checkbox(&mut cfg.color_organ, "Color Organ");
            });
        }

        // === Group name (editable) ===
        {
            let wc = self.state.working_copy.as_mut().unwrap();
            let cfg = &mut wc.groups[group_idx].config;
            ui.horizontal(|ui| {
                ui.label("Group name:");
                let mut name = cfg.group.as_ref().map(|k| k.0.clone()).unwrap_or_default();
                if ui.text_edit_singleline(&mut name).changed() {
                    cfg.group = if name.is_empty() {
                        None
                    } else {
                        Some(FixtureGroupKey(name))
                    };
                }
            });
        }

        // === Group options (read-only) ===
        {
            let wc = self.state.working_copy.as_ref().unwrap();
            let cfg = &wc.groups[group_idx].config;
            let patcher = self.patchers.iter().find(|p| p.name.0 == cfg.fixture);
            if let Some(patcher) = patcher {
                let group_opts = (patcher.group_options)();
                if !group_opts.is_empty() {
                    ui.add_space(4.0);
                    ui.label("Group Options");
                    egui::Grid::new("group_options_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for (opt_key, _) in &group_opts {
                                let val = cfg.options.get_string(opt_key).unwrap_or_default();
                                ui.label(opt_key);
                                ui.label(&val);
                                ui.end_row();
                            }
                        });
                }
            }
        }

        // === Fixtures table (editable addresses, universe, mirror) ===
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Fixtures");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Add").clicked() {
                    let wc = self.state.working_copy.as_ref().unwrap();
                    let form = AddFixtureForm::new_for_group(&wc.groups[group_idx], self.patchers);
                    self.state.mode = PanelMode::AddFixture(form);
                }
            });
        });

        // Build address map for collision detection.
        let addr_map = {
            let wc = self.state.working_copy.as_ref().unwrap();
            build_address_map(wc)
        };

        let mut fixture_swap: Option<(usize, usize)> = None;
        let mut fixture_delete: Option<usize> = None;

        {
            let wc = self.state.working_copy.as_mut().unwrap();
            let group = &mut wc.groups[group_idx];
            let num_patches = group.config.patches.len();

            egui::Grid::new("fixtures_grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.label("");
                    ui.label("#");
                    ui.label("Addr");
                    ui.label("Uni");
                    ui.label("Ch");
                    ui.label("Mir");
                    ui.label("");
                    ui.end_row();

                    for i in 0..num_patches {
                        let ch_count = group.channel_counts.get(i).copied().unwrap_or(0);

                        // Reorder buttons.
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(i > 0, egui::Button::new("Up").small())
                                .clicked()
                            {
                                fixture_swap = Some((i, i - 1));
                            }
                            if ui
                                .add_enabled(i < num_patches - 1, egui::Button::new("Dn").small())
                                .clicked()
                            {
                                fixture_swap = Some((i, i + 1));
                            }
                        });

                        ui.label(format!("{}", i + 1));

                        // Editable address.
                        let block = &mut group.config.patches[i];
                        let (start, _count) = block.start_count();
                        let mut addr_str = start
                            .map(|a| format!("{a}"))
                            .unwrap_or_else(|| "-".to_string());

                        // Check collision.
                        let has_collision = start
                            .map(|a| {
                                (0..ch_count).any(|ch| {
                                    collision_at(&addr_map, block.universe, a.dmx_index() + 1 + ch)
                                        .is_some()
                                })
                            })
                            .unwrap_or(false);

                        let text_edit =
                            egui::TextEdit::singleline(&mut addr_str).desired_width(40.0);
                        let response = ui.add(text_edit);
                        if has_collision {
                            response.clone().on_hover_text("DMX address collision!");
                        }
                        if response.changed() {
                            if let Ok(v) = addr_str.parse::<usize>() {
                                block.addr = Some(DmxAddrConfig::Single(DmxAddr::new(v)));
                            } else if addr_str == "-" || addr_str.is_empty() {
                                block.addr = None;
                            }
                        }

                        // Editable universe.
                        let mut uni_str = format!("{}", block.universe);
                        let uni_edit = egui::TextEdit::singleline(&mut uni_str).desired_width(25.0);
                        if ui.add(uni_edit).changed()
                            && let Ok(v) = uni_str.parse::<usize>() {
                                block.universe = v;
                            }

                        ui.label(format!("{ch_count}"));

                        // Editable mirror.
                        ui.checkbox(&mut block.mirror, "");

                        // Delete button.
                        if ui.button("x").clicked() {
                            fixture_delete = Some(i);
                        }

                        ui.end_row();
                    }
                });
        }

        // Apply fixture reorder.
        if let Some((a, b)) = fixture_swap {
            let wc = self.state.working_copy.as_mut().unwrap();
            let group = &mut wc.groups[group_idx];
            group.config.patches.swap(a, b);
            group.channel_counts.swap(a, b);
        }

        // Apply fixture delete.
        if let Some(idx) = fixture_delete {
            let wc = self.state.working_copy.as_mut().unwrap();
            let group = &mut wc.groups[group_idx];
            group.config.patches.remove(idx);
            group.channel_counts.remove(idx);
        }

        // === Patch options (read-only) ===
        {
            let wc = self.state.working_copy.as_ref().unwrap();
            let cfg = &wc.groups[group_idx].config;
            let patcher = self.patchers.iter().find(|p| p.name.0 == cfg.fixture);
            if let Some(patcher) = patcher {
                let patch_opts = (patcher.patch_options)();
                if !patch_opts.is_empty() {
                    ui.add_space(4.0);
                    ui.label("Patch Options (per fixture)");
                    egui::Grid::new("patch_options_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("#");
                            for (opt_key, _) in &patch_opts {
                                ui.label(opt_key);
                            }
                            ui.end_row();

                            for (i, block) in cfg.patches.iter().enumerate() {
                                ui.label(format!("{}", i + 1));
                                for (opt_key, _) in &patch_opts {
                                    let val = block.options.get_string(opt_key).unwrap_or_default();
                                    ui.label(&val);
                                }
                                ui.end_row();
                            }
                        });
                }
            }
        }

        // === Add fixture inline form ===
        if matches!(self.state.mode, PanelMode::AddFixture(_)) {
            ui.separator();
            self.render_add_fixture_form(ui, group_idx);
        }
    }

    // -----------------------------------------------------------------------
    // Add group form
    // -----------------------------------------------------------------------

    fn render_add_group_form(&mut self, ui: &mut egui::Ui) {
        ui.heading("Add New Group");
        ui.separator();

        // We need to take the form out temporarily to avoid borrow issues.
        let PanelMode::AddGroup(ref mut form) = self.state.mode else {
            return;
        };

        // Fixture type combo box.
        let prev_idx = form.fixture_type_idx;
        let selected_name = self
            .patchers
            .get(form.fixture_type_idx)
            .map(|p| p.name.0)
            .unwrap_or("(none)");

        egui::ComboBox::from_label("Fixture Type")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for (i, p) in self.patchers.iter().enumerate() {
                    ui.selectable_value(&mut form.fixture_type_idx, i, p.name.0);
                }
            });

        if form.fixture_type_idx != prev_idx {
            form.sync_options(self.patchers);
        }

        // Show channel count hint.
        if let Some(patcher) = self.patchers.get(form.fixture_type_idx)
            && let Ok(cfg) = (patcher.create_patch)(Options::default(), Options::default())
                && cfg.channel_count > 0 {
                    ui.label(format!(
                        "({} DMX channel{} per fixture)",
                        cfg.channel_count,
                        if cfg.channel_count > 1 { "s" } else { "" }
                    ));
                }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Group Name:");
            ui.text_edit_singleline(&mut form.group_name);
        });
        ui.label("(optional — defaults to fixture type)");

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut form.channel, "Channel");
            ui.checkbox(&mut form.color_organ, "Color Organ");
        });

        // Group options (editable in creation flow).
        let patcher = self.patchers.get(form.fixture_type_idx);
        let mut all_valid = true;

        if let Some(patcher) = patcher {
            let menu = (patcher.group_options)();
            if !menu.is_empty() {
                ui.add_space(4.0);
                ui.label("Group Options");
                for (menu_key, menu_opt) in &menu {
                    if let Some(entry) = form.group_options.iter_mut().find(|(k, _)| k == menu_key)
                    {
                        render_option_widget(ui, menu_key, menu_opt, &mut entry.1);
                        if let Err(msg) = validate_option(menu_opt, &entry.1) {
                            ui.colored_label(egui::Color32::RED, &msg);
                            all_valid = false;
                        }
                    }
                }
            }
        }

        // First fixture address.
        ui.add_space(4.0);
        ui.label("First Fixture");
        ui.horizontal(|ui| {
            ui.label("DMX Address:");
            ui.text_edit_singleline(&mut form.first_addr)
                .on_hover_text("1-512");
            ui.label("Universe:");
            ui.text_edit_singleline(&mut form.first_universe)
                .on_hover_text("0+");
        });

        let addr_valid = form.first_addr.parse::<usize>().is_ok();
        let uni_valid = form.first_universe.parse::<usize>().is_ok();

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                self.state.mode = PanelMode::View;
            }
            if ui
                .add_enabled(
                    all_valid && addr_valid && uni_valid,
                    egui::Button::new("Add Group"),
                )
                .clicked()
            {
                self.commit_add_group();
            }
        });
    }

    fn commit_add_group(&mut self) {
        let PanelMode::AddGroup(ref form) = self.state.mode else {
            return;
        };

        let patcher = match self.patchers.get(form.fixture_type_idx) {
            Some(p) => p,
            None => return,
        };

        let addr: usize = match form.first_addr.parse() {
            Ok(v) => v,
            Err(_) => return,
        };
        let universe: usize = form.first_universe.parse().unwrap_or(0);

        let group_options = build_options_from_form(&form.group_options);

        let group_name = if form.group_name.is_empty() {
            None
        } else {
            Some(FixtureGroupKey(form.group_name.clone()))
        };

        let config = FixtureGroupConfig {
            fixture: patcher.name.0.to_string(),
            group: group_name,
            channel: form.channel,
            color_organ: form.color_organ,
            patches: vec![PatchBlock {
                addr: Some(DmxAddrConfig::Single(DmxAddr::new(addr))),
                universe,
                mirror: false,
                options: Options::default(),
            }],
            options: group_options,
        };

        let working_group = PatchWorkingCopy::resolve_group(&config, self.patchers);
        let wc = self.state.working_copy.as_mut().unwrap();
        let new_idx = wc.groups.len();
        wc.groups.push(working_group);
        self.state.selected_group = Some(new_idx);
        self.state.mode = PanelMode::View;
    }

    // -----------------------------------------------------------------------
    // Add fixture inline form
    // -----------------------------------------------------------------------

    fn render_add_fixture_form(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        ui.label("Add Fixture");

        let PanelMode::AddFixture(ref mut form) = self.state.mode else {
            return;
        };

        let wc = self.state.working_copy.as_ref().unwrap();
        let fixture_type = &wc.groups[group_idx].config.fixture;
        let patcher = self.patchers.iter().find(|p| p.name.0 == *fixture_type);
        let mut all_valid = true;

        ui.horizontal(|ui| {
            ui.label("Addr:");
            ui.text_edit_singleline(&mut form.addr)
                .on_hover_text("1-512");
            ui.label("Uni:");
            let uni_edit = egui::TextEdit::singleline(&mut form.universe).desired_width(25.0);
            ui.add(uni_edit);
            ui.label("Count:");
            let count_edit = egui::TextEdit::singleline(&mut form.count).desired_width(25.0);
            ui.add(count_edit);
            ui.checkbox(&mut form.mirror, "Mirror");
        });

        // Patch-level options (editable in add flow).
        if let Some(patcher) = patcher {
            let patch_menu = (patcher.patch_options)();
            if !patch_menu.is_empty() {
                for (opt_key, opt_type) in &patch_menu {
                    if let Some(entry) = form.patch_options.iter_mut().find(|(k, _)| k == opt_key) {
                        render_option_widget(ui, opt_key, opt_type, &mut entry.1);
                        if let Err(msg) = validate_option(opt_type, &entry.1) {
                            ui.colored_label(egui::Color32::RED, &msg);
                            all_valid = false;
                        }
                    }
                }
            }
        }

        let addr_valid = form.addr.parse::<usize>().is_ok();
        let count_valid = form.count.parse::<usize>().map(|c| c >= 1).unwrap_or(false);

        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                self.state.mode = PanelMode::View;
            }
            if ui
                .add_enabled(
                    all_valid && addr_valid && count_valid,
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                self.commit_add_fixture(group_idx);
            }
        });
    }

    fn commit_add_fixture(&mut self, group_idx: usize) {
        let PanelMode::AddFixture(ref form) = self.state.mode else {
            return;
        };

        let start_addr: usize = match form.addr.parse() {
            Ok(v) => v,
            Err(_) => return,
        };
        let universe: usize = form.universe.parse().unwrap_or(0);
        let count: usize = form.count.parse().unwrap_or(1).max(1);
        let mirror = form.mirror;
        let patch_options = build_options_from_form(&form.patch_options);

        let wc = self.state.working_copy.as_mut().unwrap();
        let group = &mut wc.groups[group_idx];
        let patcher = self
            .patchers
            .iter()
            .find(|p| p.name.0 == group.config.fixture);

        // Resolve channel count for the new fixture(s).
        let ch_count = patcher
            .and_then(|p| {
                (p.create_patch)(group.config.options.clone(), patch_options.clone())
                    .ok()
                    .map(|c| c.channel_count)
            })
            .unwrap_or(0);

        if count == 1 {
            group.config.patches.push(PatchBlock {
                addr: Some(DmxAddrConfig::Single(DmxAddr::new(start_addr))),
                universe,
                mirror,
                options: patch_options,
            });
            group.channel_counts.push(ch_count);
        } else {
            // Range add: create `count` fixtures at consecutive addresses.
            for c in 0..count {
                let addr = start_addr + c * ch_count;
                group.config.patches.push(PatchBlock {
                    addr: Some(DmxAddrConfig::Single(DmxAddr::new(addr))),
                    universe,
                    mirror,
                    options: patch_options.clone(),
                });
                group.channel_counts.push(ch_count);
            }
        }

        self.state.mode = PanelMode::View;
    }

    // -----------------------------------------------------------------------
    // DMX Address Map (WP6)
    // -----------------------------------------------------------------------

    fn render_address_map(
        &self,
        ui: &mut egui::Ui,
        wc: &PatchWorkingCopy,
        addr_map: &BTreeMap<(usize, usize), Vec<AddrMapEntry>>,
    ) {
        ui.heading("DMX Map");
        ui.separator();

        if wc.groups.is_empty() || addr_map.is_empty() {
            ui.label("No addresses in use.");
            return;
        }

        // Determine universes in use.
        let universes: Vec<usize> = addr_map
            .keys()
            .map(|(u, _)| *u)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        for universe in &universes {
            ui.label(format!("Universe {universe}"));

            // Collect contiguous ranges for this universe.
            let entries: Vec<_> = addr_map.range((*universe, 1)..=(*universe, 512)).collect();

            if entries.is_empty() {
                ui.label("  (empty)");
                continue;
            }

            // Group into contiguous runs of the same group.
            let mut ranges: Vec<(usize, usize, String, bool)> = Vec::new(); // (start, end, name, collision)

            for entry in &entries {
                let addr = entry.0.1;
                let occupants = entry.1;
                let name = &occupants[0].group_name;
                let is_collision = occupants.len() > 1;

                if let Some(last) = ranges.last_mut()
                    && last.2 == *name && last.1 + 1 == addr && last.3 == is_collision {
                        last.1 = addr;
                        continue;
                    }
                ranges.push((addr, addr, name.clone(), is_collision));
            }

            for (start, end, name, is_collision) in &ranges {
                let range_str = if start == end {
                    format!("{start:>3}")
                } else {
                    format!("{start:>3}-{end}")
                };

                let label = format!("{range_str}  {name}");
                if *is_collision {
                    ui.colored_label(self.status_colors.error, &label)
                        .on_hover_text("DMX address collision!");
                } else {
                    ui.label(&label);
                }
            }

            ui.add_space(4.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Option widget rendering (for creation forms)
// ---------------------------------------------------------------------------

fn render_option_widget(ui: &mut egui::Ui, key: &str, opt: &PatchOption, value: &mut String) {
    match opt {
        PatchOption::Bool => {
            let mut checked = value == "true";
            if ui.checkbox(&mut checked, key).changed() {
                *value = checked.to_string();
            }
        }
        PatchOption::Select(choices) => {
            egui::ComboBox::from_label(key)
                .selected_text(value.as_str())
                .show_ui(ui, |ui| {
                    for choice in choices {
                        ui.selectable_value(value, choice.clone(), choice);
                    }
                });
        }
        PatchOption::Int | PatchOption::Url | PatchOption::SocketAddr => {
            ui.horizontal(|ui| {
                ui.label(key);
                ui.text_edit_singleline(value);
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixture::patch::PATCHERS;

    fn test_patchers() -> Vec<Patcher> {
        PATCHERS.iter().cloned().collect()
    }

    fn test_snapshot_empty() -> PatchSnapshot {
        PatchSnapshot { groups: vec![] }
    }

    fn dimmer_block(addr: usize) -> PatchBlock {
        PatchBlock {
            addr: Some(DmxAddrConfig::Single(DmxAddr::new(addr))),
            universe: 0,
            mirror: false,
            options: Options::default(),
        }
    }

    fn dimmer_group(name: Option<&str>, addrs: &[usize]) -> FixtureGroupConfig {
        FixtureGroupConfig {
            fixture: "Dimmer".to_string(),
            group: name.map(|n| FixtureGroupKey(n.to_string())),
            channel: true,
            color_organ: false,
            patches: addrs.iter().map(|&a| dimmer_block(a)).collect(),
            options: Options::default(),
        }
    }

    fn test_snapshot_with_groups() -> PatchSnapshot {
        PatchSnapshot {
            groups: vec![
                dimmer_group(None, &[1]),
                dimmer_group(Some("BackDimmer"), &[10, 11]),
            ],
        }
    }

    // -- WP3 tests (retained) --

    #[test]
    fn working_copy_from_empty_snapshot() {
        let snapshot = test_snapshot_empty();
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        assert!(wc.groups.is_empty());
    }

    #[test]
    fn working_copy_resolves_channel_counts() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        assert_eq!(wc.groups.len(), 2);
        assert_eq!(wc.groups[0].channel_counts, vec![1]);
        assert_eq!(wc.groups[1].channel_counts, vec![1, 1]);
    }

    #[test]
    fn working_copy_unknown_fixture_gets_zero_channels() {
        let snapshot = PatchSnapshot {
            groups: vec![FixtureGroupConfig {
                fixture: "NonexistentFixture".to_string(),
                group: None,
                channel: true,
                color_organ: false,
                patches: vec![dimmer_block(1)],
                options: Options::default(),
            }],
        };
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        assert_eq!(wc.groups[0].channel_counts, vec![0]);
    }

    // -- WP4 tests --

    #[test]
    fn add_group_to_working_copy() {
        let snapshot = test_snapshot_empty();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        let config = dimmer_group(Some("NewGroup"), &[50]);
        let working_group = PatchWorkingCopy::resolve_group(&config, &patchers);
        wc.groups.push(working_group);

        assert_eq!(wc.groups.len(), 1);
        assert_eq!(wc.groups[0].config.key(), "NewGroup");
        assert_eq!(wc.groups[0].channel_counts, vec![1]);
    }

    #[test]
    fn remove_group_from_working_copy() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        assert_eq!(wc.groups.len(), 2);
        wc.groups.remove(0);
        assert_eq!(wc.groups.len(), 1);
        assert_eq!(wc.groups[0].config.key(), "BackDimmer");
    }

    #[test]
    fn add_fixture_to_group() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        let group = &mut wc.groups[0];
        assert_eq!(group.config.patches.len(), 1);

        group.config.patches.push(dimmer_block(5));
        group.channel_counts.push(1);

        assert_eq!(group.config.patches.len(), 2);
        assert_eq!(group.channel_counts.len(), 2);
    }

    #[test]
    fn remove_fixture_from_group() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        let group = &mut wc.groups[1];
        assert_eq!(group.config.patches.len(), 2);

        group.config.patches.remove(0);
        group.channel_counts.remove(0);

        assert_eq!(group.config.patches.len(), 1);
        assert_eq!(group.channel_counts.len(), 1);
    }

    #[test]
    fn reorder_fixtures_preserves_sync() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        let group = &mut wc.groups[1];
        // BackDimmer has patches at addr 10 and 11.
        let addr_before_0 = group.config.patches[0].start_count().0.unwrap().dmx_index();
        let addr_before_1 = group.config.patches[1].start_count().0.unwrap().dmx_index();

        group.config.patches.swap(0, 1);
        group.channel_counts.swap(0, 1);

        assert_eq!(
            group.config.patches[0].start_count().0.unwrap().dmx_index(),
            addr_before_1
        );
        assert_eq!(
            group.config.patches[1].start_count().0.unwrap().dmx_index(),
            addr_before_0
        );
        // Channel counts still match patches.
        assert_eq!(group.channel_counts.len(), group.config.patches.len());
    }

    #[test]
    fn configs_extracts_all_group_configs() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        let configs = wc.configs();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].fixture, "Dimmer");
        assert_eq!(configs[1].key(), "BackDimmer");
    }

    // -- WP4 validation tests --

    #[test]
    fn validate_int_option() {
        assert!(validate_option(&PatchOption::Int, "42").is_ok());
        assert!(validate_option(&PatchOption::Int, "-1").is_ok());
        assert!(validate_option(&PatchOption::Int, "abc").is_err());
    }

    #[test]
    fn validate_bool_always_ok() {
        assert!(validate_option(&PatchOption::Bool, "anything").is_ok());
    }

    #[test]
    fn validate_select_always_ok() {
        let opt = PatchOption::Select(vec!["a".into(), "b".into()]);
        assert!(validate_option(&opt, "a").is_ok());
        assert!(validate_option(&opt, "z").is_ok()); // Select validation is at widget level.
    }

    // -- WP5 tests --

    #[test]
    fn swap_groups_reorders() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);

        assert_eq!(wc.groups[0].config.key(), "Dimmer");
        assert_eq!(wc.groups[1].config.key(), "BackDimmer");

        wc.groups.swap(0, 1);

        assert_eq!(wc.groups[0].config.key(), "BackDimmer");
        assert_eq!(wc.groups[1].config.key(), "Dimmer");
    }

    // -- WP6 tests --

    #[test]
    fn address_map_build() {
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        let map = build_address_map(&wc);

        // Dimmer at addr 1 (1 channel) -> (0, 1)
        assert!(map.contains_key(&(0, 1)));
        assert_eq!(map[&(0, 1)].len(), 1);
        assert_eq!(map[&(0, 1)][0].group_name, "Dimmer");

        // BackDimmer at addr 10, 11 -> (0, 10), (0, 11)
        assert!(map.contains_key(&(0, 10)));
        assert!(map.contains_key(&(0, 11)));
    }

    #[test]
    fn address_map_detects_collision() {
        let snapshot = PatchSnapshot {
            groups: vec![
                dimmer_group(Some("A"), &[1]),
                dimmer_group(Some("B"), &[1]), // same address!
            ],
        };
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        let map = build_address_map(&wc);

        // Address 1 should have 2 occupants.
        assert_eq!(map[&(0, 1)].len(), 2);
        let collision = collision_at(&map, 0, 1);
        assert!(collision.is_some());
        assert!(collision.unwrap().contains("A"));
    }

    #[test]
    fn address_map_no_collision_different_universes() {
        let snapshot = PatchSnapshot {
            groups: vec![
                dimmer_group(Some("A"), &[1]),
                FixtureGroupConfig {
                    fixture: "Dimmer".to_string(),
                    group: Some(FixtureGroupKey("B".to_string())),
                    channel: true,
                    color_organ: false,
                    patches: vec![PatchBlock {
                        addr: Some(DmxAddrConfig::Single(DmxAddr::new(1))),
                        universe: 1, // different universe
                        mirror: false,
                        options: Options::default(),
                    }],
                    options: Options::default(),
                },
            ],
        };
        let patchers = test_patchers();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &patchers);
        let map = build_address_map(&wc);

        // Same address, different universes — no collision.
        assert_eq!(map[&(0, 1)].len(), 1);
        assert_eq!(map[&(1, 1)].len(), 1);
    }

    #[test]
    fn build_options_from_form_values() {
        let entries = vec![
            ("flag".to_string(), "true".to_string()),
            ("count".to_string(), "42".to_string()),
            ("name".to_string(), "hello".to_string()),
        ];
        let opts = build_options_from_form(&entries);
        assert_eq!(opts.get_bool("flag"), Some(true));
        assert_eq!(opts.get_string("count").as_deref(), Some("42"));
        assert_eq!(opts.get_string("name").as_deref(), Some("hello"));
    }

    // -- Snapshot tests --

    use crate::control::mock::auto_respond_client;
    use crate::ui_util::{ErrorModal, StatusColors};
    use egui_kittest::Harness;

    fn test_status_colors() -> StatusColors {
        StatusColors::default()
    }

    #[test]
    fn render_empty_patch() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_empty();
        let patchers = test_patchers();
        let status_colors = test_status_colors();
        let mut error_modal = ErrorModal::default();
        let mut state = PatchPanelState::new();

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
                status_colors: &status_colors,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("patch_panel_empty");
    }

    #[test]
    fn render_with_groups() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let status_colors = test_status_colors();
        let mut error_modal = ErrorModal::default();
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
                status_colors: &status_colors,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("patch_panel_with_groups");
    }

    #[test]
    fn render_second_group_selected() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let status_colors = test_status_colors();
        let mut error_modal = ErrorModal::default();
        let mut state = PatchPanelState::new();
        state.selected_group = Some(1);

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
                status_colors: &status_colors,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("patch_panel_second_group");
    }

    #[test]
    fn render_add_group_form() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let status_colors = test_status_colors();
        let mut error_modal = ErrorModal::default();
        let mut state = PatchPanelState::new();
        let mut form = AddGroupForm::new();
        form.sync_options(&patchers);
        state.mode = PanelMode::AddGroup(form);

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
                status_colors: &status_colors,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("patch_panel_add_group");
    }

    #[test]
    fn render_with_dmx_map() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let status_colors = test_status_colors();
        let mut error_modal = ErrorModal::default();
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);
        state.show_address_map = true;

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    error_modal: &mut error_modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
                status_colors: &status_colors,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot("patch_panel_dmx_map");
    }
}
