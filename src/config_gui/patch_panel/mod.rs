mod address_map;
mod widgets;
mod working_copy;

use eframe::egui;

use crate::config::{DmxAddrConfig, FixtureGroupConfig, FixtureGroupKey, Options, PatchBlock};
use crate::control::MetaCommand;
use crate::dmx::DmxAddr;
use crate::fixture::patch::{PatchOption, Patcher};
use crate::gui_state::PatchSnapshot;
use crate::ui_util::{GuiContext, STATUS_COLORS, cancel_button, confirm_button, confirm_button_enabled};

use address_map::{AddressMap, UniverseAddress};
use widgets::{
    build_options_from_form, default_for_option, render_address_map,
    render_option_widget, validate_option,
};
use working_copy::PatchWorkingCopy;

// ---------------------------------------------------------------------------
// Form state
// ---------------------------------------------------------------------------

struct AddGroupForm {
    fixture_type_idx: usize,
    group_name: String,
    channel: bool,
    group_options: Vec<(String, String)>,
}

impl AddGroupForm {
    fn new() -> Self {
        Self {
            fixture_type_idx: 0,
            group_name: String::new(),
            channel: true,
            group_options: Vec::new(),
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

struct AddFixtureForm {
    addr: String,
    universe: String,
    count: String,
    mirror: bool,
    patch_options: Vec<(String, String)>,
}

impl AddFixtureForm {
    fn new_for_group(
        group: &working_copy::WorkingGroup,
        patcher: &Patcher,
        addr_map: &AddressMap,
    ) -> Self {
        let patch_options: Vec<(String, String)> = (patcher.patch_options)()
            .iter()
            .map(|(key, opt)| (key.clone(), default_for_option(opt)))
            .collect();

        let default_patch_opts = build_options_from_form(&patch_options);
        let default_ch_count =
            (patcher.create_patch)(group.config.options.clone(), default_patch_opts)
                .map(|c| c.channel_count)
                .unwrap_or(0);

        let start_after = group
            .config
            .patches
            .last()
            .and_then(|b| {
                let (start, count) = b.start_count();
                let ch = group.channel_counts.last().copied().unwrap_or(0);
                start.map(|a| a.dmx_index() + 1 + ch * count)
            })
            .unwrap_or(1);

        let next_addr = if default_ch_count > 0 {
            addr_map
                .find_available(0, default_ch_count, start_after)
                .map(|a| a.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

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
// Panel state
// ---------------------------------------------------------------------------

enum PanelMode {
    View,
    AddGroup(AddGroupForm),
    AddFixture(AddFixtureForm),
}

pub struct PatchPanelState {
    working_copy: Option<PatchWorkingCopy>,
    selected_group: Option<usize>,
    show_address_map: bool,
    mode: PanelMode,
    /// Group index pending delete confirmation via modal.
    pending_delete: Option<usize>,
}

impl PatchPanelState {
    pub fn new() -> Self {
        Self {
            working_copy: None,
            selected_group: None,
            show_address_map: false,
            mode: PanelMode::View,
            pending_delete: None,
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
}

impl PatchPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        if self.state.working_copy.is_none() {
            self.state.working_copy = Some(PatchWorkingCopy::from_snapshot(
                self.snapshot,
                self.patchers,
            ));
        }

        // Bottom bar — content depends on mode.
        egui::TopBottomPanel::bottom("patch_buttons").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                match &self.state.mode {
                    PanelMode::AddGroup(_) => {
                        // Form buttons handled inline — just show cancel here.
                        // The confirm button is rendered by render_add_group_form
                        // since it needs access to validation state.
                    }
                    PanelMode::AddFixture(_) => {
                        // Same — handled inline in render_add_fixture_form.
                    }
                    _ => {
                        if confirm_button(ui, "Apply") {
                            let Some(wc) = self.state.working_copy.as_ref() else { return };
                            let configs = wc.configs();
                            if self.ctx.send_command(MetaCommand::Repatch(configs)).is_ok() {
                                self.state.working_copy = None;
                                self.state.mode = PanelMode::View;
                                self.ctx.modal.show(
                                    "Patch Applied",
                                    "Patch configuration updated successfully.",
                                );
                            }
                        }
                        if cancel_button(ui, "Revert") {
                            self.state.working_copy = None;
                            self.state.selected_group = None;
                            self.state.mode = PanelMode::View;
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.toggle_value(&mut self.state.show_address_map, "DMX Map");
                            },
                        );
                    }
                }
            });
        });

        // DMX Address Map floating window.
        if self.state.show_address_map {
            let Some(wc) = self.state.working_copy.as_ref() else { return };
            let addr_map = AddressMap::from_working_copy(wc);
            let mut show = self.state.show_address_map;
            egui::Window::new("DMX Address Map")
                .open(&mut show)
                .resizable(true)
                .default_width(250.0)
                .vscroll(true)
                .show(ui.ctx(), |ui| {
                    render_address_map(ui, wc, &addr_map);
                });
            self.state.show_address_map = show;
        }

        // Main content — submenu modes hide the header and group list.
        match &self.state.mode {
            PanelMode::AddGroup(_) => self.render_add_group_form(ui),
            _ => {
                ui.heading("Groups");
                ui.separator();
                self.render_main_view(ui);
            }
        }

        // Delete group confirmation modal.
        if let Some(group_idx) = self.state.pending_delete {
            let (key, fix_count) = self
                .state
                .working_copy
                .as_ref()
                .and_then(|wc| {
                    wc.groups.get(group_idx).map(|g| {
                        (g.config.key().to_string(), g.config.patches.len())
                    })
                })
                .unwrap_or_default();

            let response =
                egui::Modal::new(egui::Id::new("delete_group_modal")).show(ui.ctx(), |ui| {
                    ui.set_width(300.0);
                    ui.heading("Delete Group");
                    ui.label(format!("Really delete {key} ({fix_count} fixtures)?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if confirm_button(ui, "Delete") {
                            let Some(wc) = self.state.working_copy.as_mut() else {
                                return;
                            };
                            wc.groups.remove(group_idx);
                            self.state.selected_group = if wc.groups.is_empty() {
                                None
                            } else {
                                Some(group_idx.min(wc.groups.len() - 1))
                            };
                            self.state.pending_delete = None;
                            ui.close();
                        }
                        if cancel_button(ui, "Cancel") {
                            self.state.pending_delete = None;
                            ui.close();
                        }
                    });
                });
            if response.should_close() {
                self.state.pending_delete = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Main view: group list + detail
    // -----------------------------------------------------------------------

    fn render_main_view(&mut self, ui: &mut egui::Ui) {
        let Some(wc) = self.state.working_copy.as_ref() else { return };

        let num_groups = wc.groups.len();
        if let Some(sel) = self.state.selected_group
            && sel >= num_groups
        {
            self.state.selected_group = Some(num_groups - 1);
        }

        // Compute channel numbers (assigned sequentially to channeled groups).
        let channel_numbers: Vec<Option<usize>> = {
            let Some(wc) = self.state.working_copy.as_ref() else { return };
            let mut ch = 0;
            wc.groups
                .iter()
                .map(|g| {
                    if g.config.channel {
                        let n = ch;
                        ch += 1;
                        Some(n)
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut swap: Option<(usize, usize)> = None;

        // Group table — no buttons in rows; drag to reorder, click to select.
        egui::ScrollArea::vertical()
            .max_height(150.0)
            .id_salt("patch_group_list")
            .show(ui, |ui| {
                let Some(wc) = self.state.working_copy.as_ref() else { return };
                let n = wc.groups.len();

                if n == 0 {
                    ui.label("No groups. Click + Add Group below.");
                    return;
                }

                let selected = self.state.selected_group;
                let selection_color = ui.style().visuals.selection.bg_fill;

                egui::Grid::new("group_table")
                    .striped(true)
                    .with_row_color(move |row, style| {
                        if row == 0 {
                            return None;
                        }
                        if selected == Some(row - 1) {
                            Some(style.visuals.selection.bg_fill)
                        } else {
                            None
                        }
                    })
                    .show(ui, |ui| {
                        ui.label("Ch");
                        ui.label("Name");
                        ui.label("Type");
                        ui.label("Count");
                        ui.end_row();

                        for i in 0..n {
                            let group = &wc.groups[i];
                            let row_top = ui.cursor().top();

                            // Channel number.
                            let ch_text = match channel_numbers.get(i).copied().flatten() {
                                Some(ch) => format!("{ch}"),
                                None => "-".to_string(),
                            };
                            ui.label(&ch_text);
                            ui.label(group.config.key());
                            ui.label(&group.config.fixture);
                            ui.label(format!("{}", group.config.patches.len()));

                            ui.end_row();

                            // Full-row interaction: click to select, drag to reorder.
                            let row_bottom = ui.cursor().top();
                            let row_rect = egui::Rect::from_x_y_ranges(
                                ui.min_rect().x_range(),
                                row_top..=row_bottom,
                            );
                            let response = ui.interact(
                                row_rect,
                                ui.id().with(("group_row", i)),
                                egui::Sense::click_and_drag(),
                            );

                            // Cursor feedback.
                            if response.dragged() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }

                            // Click → select.
                            if response.clicked() {
                                self.state.selected_group = Some(i);
                                if matches!(self.state.mode, PanelMode::AddFixture(_)) {
                                    self.state.mode = PanelMode::View;
                                }
                            }

                            // Drag → reorder via DnD payload.
                            response.dnd_set_drag_payload(i);

                            if let Some(source_idx) = response.dnd_release_payload::<usize>() {
                                swap = Some((*source_idx, i));
                            }

                            // Visual: drop indicator line when hovering during drag.
                            if let Some(source_idx) = response.dnd_hover_payload::<usize>() {
                                let stroke = egui::Stroke::new(2.0, selection_color);
                                let y = if *source_idx <= i {
                                    row_rect.bottom()
                                } else {
                                    row_rect.top()
                                };
                                ui.painter().hline(row_rect.x_range(), y, stroke);
                            }
                        }
                    });
            });

        // + Add Group button below the list.
        if ui.button("+ Add Group").clicked() {
            let mut form = AddGroupForm::new();
            form.sync_options(self.patchers);
            self.state.mode = PanelMode::AddGroup(form);
        }

        // Apply swap.
        if let Some((a, b)) = swap {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
            wc.groups.swap(a, b);
            if self.state.selected_group == Some(a) {
                self.state.selected_group = Some(b);
            } else if self.state.selected_group == Some(b) {
                self.state.selected_group = Some(a);
            }
        }

        ui.separator();

        // Detail view.
        if let Some(sel) = self.state.selected_group {
            let Some(wc) = self.state.working_copy.as_ref() else { return };
            if sel < wc.groups.len() {
                self.render_detail(ui, sel);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Detail view
    // -----------------------------------------------------------------------

    fn render_detail(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        self.render_detail_editable_fields(ui, group_idx);

        // Delete group button.
        if cancel_button(ui, "Delete Group") {
            self.state.pending_delete = Some(group_idx);
        }
        self.render_detail_group_options(ui, group_idx);
        self.render_fixtures_table(ui, group_idx);

        if matches!(self.state.mode, PanelMode::AddFixture(_)) {
            ui.separator();
            self.render_add_fixture_form(ui, group_idx);
        }
    }

    fn render_detail_editable_fields(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
            let cfg = &mut wc.groups[group_idx].config;
            ui.checkbox(&mut cfg.channel, "Channel");
        }
        {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
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
    }

    fn render_detail_group_options(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        let Some(wc) = self.state.working_copy.as_ref() else { return };
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

    fn render_fixtures_table(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        ui.add_space(8.0);

        let addr_map = {
            let Some(wc) = self.state.working_copy.as_ref() else { return };
            AddressMap::from_working_copy(wc)
        };

        ui.horizontal(|ui| {
            ui.label("Fixtures");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ Add").clicked() {
                    let Some(wc) = self.state.working_copy.as_ref() else { return };
                    let fixture_type = &wc.groups[group_idx].config.fixture;
                    match self.patchers.iter().find(|p| p.name.0 == *fixture_type) {
                        Some(patcher) => {
                            let form = AddFixtureForm::new_for_group(
                                &wc.groups[group_idx],
                                patcher,
                                &addr_map,
                            );
                            self.state.mode = PanelMode::AddFixture(form);
                        }
                        None => {
                            self.ctx
                                .report_error(format!("Unknown fixture type: {fixture_type}"));
                        }
                    }
                }
            });
        });

        let patch_opts: Vec<(String, PatchOption)> = {
            let Some(wc) = self.state.working_copy.as_ref() else { return };
            let fixture_type = &wc.groups[group_idx].config.fixture;
            self.patchers
                .iter()
                .find(|p| p.name.0 == *fixture_type)
                .map(|p| (p.patch_options)())
                .unwrap_or_default()
        };

        let mut fixture_swap: Option<(usize, usize)> = None;
        let mut fixture_delete: Option<usize> = None;

        {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
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
                    for (opt_key, _) in &patch_opts {
                        ui.label(opt_key);
                    }
                    ui.label("");
                    ui.end_row();

                    for i in 0..num_patches {
                        let ch_count = group.channel_counts.get(i).copied().unwrap_or(0);

                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(i > 0, egui::Button::new("⏶").small())
                                .clicked()
                            {
                                fixture_swap = Some((i, i - 1));
                            }
                            if ui
                                .add_enabled(
                                    i < num_patches - 1,
                                    egui::Button::new("⏷").small(),
                                )
                                .clicked()
                            {
                                fixture_swap = Some((i, i + 1));
                            }
                        });

                        ui.label(format!("{}", i + 1));

                        let block = &mut group.config.patches[i];
                        let (start, _count) = block.start_count();
                        let mut addr_str = start.map(|a| format!("{a}")).unwrap_or_default();

                        let has_collision = start
                            .map(|a| {
                                (0..ch_count).any(|ch| {
                                    addr_map
                                        .collision_at(UniverseAddress {
                                            universe: block.universe,
                                            address: a.dmx_index() + 1 + ch,
                                        })
                                        .is_some()
                                })
                            })
                            .unwrap_or(false);

                        let addr_invalid = start.map(|a| a.validate().is_err()).unwrap_or(false);

                        let text_edit =
                            egui::TextEdit::singleline(&mut addr_str).desired_width(40.0);
                        let response = ui.add(text_edit);
                        if has_collision {
                            response.clone().on_hover_text("DMX address collision!");
                        } else if addr_invalid {
                            response.clone().on_hover_text("Address must be 1-512");
                        }
                        if response.changed() {
                            let digits: String =
                                addr_str.chars().filter(|c| c.is_ascii_digit()).collect();
                            if let Ok(v) = digits.parse::<usize>() {
                                block.addr = Some(DmxAddrConfig::Single(DmxAddr::new(v)));
                            } else {
                                block.addr = None;
                            }
                        }

                        let mut uni_str = format!("{}", block.universe);
                        let uni_edit = egui::TextEdit::singleline(&mut uni_str).desired_width(25.0);
                        if ui.add(uni_edit).changed()
                            && let Ok(v) = uni_str.parse::<usize>()
                        {
                            block.universe = v;
                        }

                        ui.label(format!("{ch_count}"));
                        ui.checkbox(&mut block.mirror, "");

                        for (opt_key, _) in &patch_opts {
                            let val = block.options.get_string(opt_key).unwrap_or_default();
                            ui.label(&val);
                        }

                        if ui.button("x").clicked() {
                            fixture_delete = Some(i);
                        }

                        ui.end_row();
                    }
                });
        }

        if let Some((a, b)) = fixture_swap {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
            let group = &mut wc.groups[group_idx];
            group.config.patches.swap(a, b);
            group.channel_counts.swap(a, b);
        }

        if let Some(idx) = fixture_delete {
            let Some(wc) = self.state.working_copy.as_mut() else { return };
            let group = &mut wc.groups[group_idx];
            group.config.patches.remove(idx);
            group.channel_counts.remove(idx);
        }
    }

    // -----------------------------------------------------------------------
    // Add group form
    // -----------------------------------------------------------------------

    fn render_add_group_form(&mut self, ui: &mut egui::Ui) {
        ui.heading("Add New Group");
        ui.separator();

        let PanelMode::AddGroup(ref mut form) = self.state.mode else {
            return;
        };

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

        if let Some(patcher) = self.patchers.get(form.fixture_type_idx)
            && let Ok(cfg) = (patcher.create_patch)(Options::default(), Options::default())
            && cfg.channel_count > 0
        {
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
        ui.checkbox(&mut form.channel, "Channel");

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
                            ui.colored_label(STATUS_COLORS.error_text, &msg);
                            all_valid = false;
                        }
                    }
                }
            }
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if confirm_button_enabled(ui, "Add Group", all_valid) {
                self.commit_add_group();
            }
            if cancel_button(ui, "Cancel") {
                self.state.mode = PanelMode::View;
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
            color_organ: false,
            patches: vec![],
            options: group_options,
        };

        let working_group = PatchWorkingCopy::resolve_group(&config, self.patchers);
        let Some(wc) = self.state.working_copy.as_mut() else { return };
        let new_idx = wc.groups.len();
        wc.groups.push(working_group);
        self.state.selected_group = Some(new_idx);
        self.state.mode = PanelMode::View;
    }

    // -----------------------------------------------------------------------
    // Add fixture form
    // -----------------------------------------------------------------------

    fn render_add_fixture_form(&mut self, ui: &mut egui::Ui, group_idx: usize) {
        ui.label("Add Fixture");

        let PanelMode::AddFixture(ref mut form) = self.state.mode else {
            return;
        };

        let Some(wc) = self.state.working_copy.as_ref() else { return };
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

        if let Some(patcher) = patcher {
            let patch_menu = (patcher.patch_options)();
            if !patch_menu.is_empty() {
                for (opt_key, opt_type) in &patch_menu {
                    if let Some(entry) = form.patch_options.iter_mut().find(|(k, _)| k == opt_key) {
                        render_option_widget(ui, opt_key, opt_type, &mut entry.1);
                        if let Err(msg) = validate_option(opt_type, &entry.1) {
                            ui.colored_label(STATUS_COLORS.error_text, &msg);
                            all_valid = false;
                        }
                    }
                }
            }
        }

        let addr_valid = form
            .addr
            .parse::<usize>()
            .map(|v| DmxAddr::new(v).validate().is_ok())
            .unwrap_or(false);
        let count_valid = form.count.parse::<usize>().map(|c| c >= 1).unwrap_or(false);

        ui.horizontal(|ui| {
            if confirm_button_enabled(ui, "Add", all_valid && addr_valid && count_valid) {
                self.commit_add_fixture(group_idx);
            }
            if cancel_button(ui, "Cancel") {
                self.state.mode = PanelMode::View;
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

        let Some(wc) = self.state.working_copy.as_mut() else { return };
        let group = &mut wc.groups[group_idx];
        let patcher = self
            .patchers
            .iter()
            .find(|p| p.name.0 == group.config.fixture);

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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use crate::fixture::patch::PatchConfig;
    use crate::fixture::prelude::FixtureType;

    // -----------------------------------------------------------------------
    // Mock patchers — no dependency on real fixture profiles
    // -----------------------------------------------------------------------

    /// Simple 1-channel fixture with no options.
    fn mock_simple_patcher() -> Patcher {
        Patcher {
            name: FixtureType("Simple"),
            create_group: |_, _| unimplemented!(),
            group_options: || vec![],
            create_patch: |_, _| {
                Ok(PatchConfig {
                    channel_count: 1,
                    render_mode: None,
                })
            },
            patch_options: || vec![],
        }
    }

    /// Fixture with group-level options covering Bool, Int, Select, and Url.
    fn mock_group_opts_patcher() -> Patcher {
        Patcher {
            name: FixtureType("GroupOpts"),
            create_group: |_, _| unimplemented!(),
            group_options: || {
                vec![
                    ("paired".into(), PatchOption::Bool),
                    ("brightness".into(), PatchOption::Int),
                    (
                        "mode".into(),
                        PatchOption::Select(vec!["Fast".into(), "Slow".into(), "Auto".into()]),
                    ),
                    ("endpoint".into(), PatchOption::Url),
                    ("addr".into(), PatchOption::SocketAddr),
                    (
                        "limit".into(),
                        PatchOption::Optional(Box::new(PatchOption::Int)),
                    ),
                ]
            },
            create_patch: |_, _| {
                Ok(PatchConfig {
                    channel_count: 4,
                    render_mode: None,
                })
            },
            patch_options: || vec![],
        }
    }

    /// Fixture with patch-level options (Select + Int).
    fn mock_patch_opts_patcher() -> Patcher {
        Patcher {
            name: FixtureType("PatchOpts"),
            create_group: |_, _| unimplemented!(),
            group_options: || vec![],
            create_patch: |_, opts| {
                let ch = match opts.get_string("variant").as_deref() {
                    Some("Wide") => 6,
                    Some("Narrow") => 3,
                    _ => 3,
                };
                Ok(PatchConfig {
                    channel_count: ch,
                    render_mode: None,
                })
            },
            patch_options: || {
                vec![
                    (
                        "variant".into(),
                        PatchOption::Select(vec!["Narrow".into(), "Wide".into()]),
                    ),
                    ("offset".into(), PatchOption::Int),
                ]
            },
        }
    }

    fn test_patchers() -> Vec<Patcher> {
        vec![
            mock_group_opts_patcher(),
            mock_patch_opts_patcher(),
            mock_simple_patcher(),
        ]
    }

    // -----------------------------------------------------------------------
    // Test data helpers
    // -----------------------------------------------------------------------

    fn test_snapshot_empty() -> PatchSnapshot {
        PatchSnapshot { groups: vec![] }
    }

    fn simple_block(addr: usize) -> PatchBlock {
        PatchBlock {
            addr: Some(DmxAddrConfig::Single(DmxAddr::new(addr))),
            universe: 0,
            mirror: false,
            options: Options::default(),
        }
    }

    fn simple_group(name: Option<&str>, addrs: &[usize]) -> FixtureGroupConfig {
        FixtureGroupConfig {
            fixture: "Simple".to_string(),
            group: name.map(|n| FixtureGroupKey(n.to_string())),
            channel: true,
            color_organ: false,
            patches: addrs.iter().map(|&a| simple_block(a)).collect(),
            options: Options::default(),
        }
    }

    fn patch_opts_block(addr: usize, variant: &str) -> PatchBlock {
        let mut options = Options::default();
        if !variant.is_empty() {
            options.set_string("variant", variant);
        }
        PatchBlock {
            addr: Some(DmxAddrConfig::Single(DmxAddr::new(addr))),
            universe: 0,
            mirror: false,
            options,
        }
    }

    fn patch_opts_group(name: Option<&str>, blocks: Vec<PatchBlock>) -> FixtureGroupConfig {
        FixtureGroupConfig {
            fixture: "PatchOpts".to_string(),
            group: name.map(|n| FixtureGroupKey(n.to_string())),
            channel: true,
            color_organ: false,
            patches: blocks,
            options: Options::default(),
        }
    }

    fn group_opts_group(name: Option<&str>) -> FixtureGroupConfig {
        let mut options = Options::default();
        options.set_bool("paired", true);
        options.set_string("brightness", "200");
        options.set_string("mode", "Fast");
        options.set_string("endpoint", "http://10.0.0.1:8080");
        FixtureGroupConfig {
            fixture: "GroupOpts".to_string(),
            group: name.map(|n| FixtureGroupKey(n.to_string())),
            channel: true,
            color_organ: false,
            patches: vec![simple_block(100)],
            options,
        }
    }

    fn test_snapshot_with_groups() -> PatchSnapshot {
        PatchSnapshot {
            groups: vec![
                simple_group(None, &[1]),
                simple_group(Some("BackSimple"), &[10, 11]),
            ],
        }
    }

    fn test_snapshot_with_options() -> PatchSnapshot {
        PatchSnapshot {
            groups: vec![
                patch_opts_group(
                    Some("FrontLights"),
                    vec![patch_opts_block(20, "Narrow"), patch_opts_block(23, "Wide")],
                ),
                group_opts_group(Some("Effects")),
                simple_group(None, &[50]),
            ],
        }
    }

    // -----------------------------------------------------------------------
    // Unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn working_copy_from_empty_snapshot() {
        let wc = PatchWorkingCopy::from_snapshot(&test_snapshot_empty(), &test_patchers());
        assert!(wc.groups.is_empty());
    }

    #[test]
    fn working_copy_resolves_channel_counts() {
        let wc = PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        assert_eq!(wc.groups.len(), 2);
        assert_eq!(wc.groups[0].channel_counts, vec![1]);
        assert_eq!(wc.groups[1].channel_counts, vec![1, 1]);
    }

    #[test]
    fn working_copy_resolves_variable_channel_counts() {
        let wc = PatchWorkingCopy::from_snapshot(&test_snapshot_with_options(), &test_patchers());
        assert_eq!(wc.groups[0].channel_counts, vec![3, 6]);
        assert_eq!(wc.groups[1].channel_counts, vec![4]);
    }

    #[test]
    fn working_copy_unknown_fixture_gets_zero_channels() {
        let snapshot = PatchSnapshot {
            groups: vec![FixtureGroupConfig {
                fixture: "NonexistentFixture".to_string(),
                group: None,
                channel: true,
                color_organ: false,
                patches: vec![simple_block(1)],
                options: Options::default(),
            }],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        assert_eq!(wc.groups[0].channel_counts, vec![0]);
    }

    #[test]
    fn add_group_to_working_copy() {
        let patchers = test_patchers();
        let mut wc = PatchWorkingCopy::from_snapshot(&test_snapshot_empty(), &patchers);
        let config = simple_group(Some("NewGroup"), &[50]);
        wc.groups
            .push(PatchWorkingCopy::resolve_group(&config, &patchers));
        assert_eq!(wc.groups.len(), 1);
        assert_eq!(wc.groups[0].config.key(), "NewGroup");
        assert_eq!(wc.groups[0].channel_counts, vec![1]);
    }

    #[test]
    fn remove_group_from_working_copy() {
        let mut wc =
            PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        assert_eq!(wc.groups.len(), 2);
        wc.groups.remove(0);
        assert_eq!(wc.groups.len(), 1);
        assert_eq!(wc.groups[0].config.key(), "BackSimple");
    }

    #[test]
    fn add_fixture_to_group() {
        let mut wc =
            PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        let group = &mut wc.groups[0];
        assert_eq!(group.config.patches.len(), 1);
        group.config.patches.push(simple_block(5));
        group.channel_counts.push(1);
        assert_eq!(group.config.patches.len(), 2);
        assert_eq!(group.channel_counts.len(), 2);
    }

    #[test]
    fn remove_fixture_from_group() {
        let mut wc =
            PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        let group = &mut wc.groups[1];
        assert_eq!(group.config.patches.len(), 2);
        group.config.patches.remove(0);
        group.channel_counts.remove(0);
        assert_eq!(group.config.patches.len(), 1);
        assert_eq!(group.channel_counts.len(), 1);
    }

    #[test]
    fn reorder_fixtures_preserves_sync() {
        let mut wc =
            PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        let group = &mut wc.groups[1];
        let addr_0 = group.config.patches[0].start_count().0.unwrap().dmx_index();
        let addr_1 = group.config.patches[1].start_count().0.unwrap().dmx_index();
        group.config.patches.swap(0, 1);
        group.channel_counts.swap(0, 1);
        assert_eq!(
            group.config.patches[0].start_count().0.unwrap().dmx_index(),
            addr_1
        );
        assert_eq!(
            group.config.patches[1].start_count().0.unwrap().dmx_index(),
            addr_0
        );
        assert_eq!(group.channel_counts.len(), group.config.patches.len());
    }

    #[test]
    fn configs_extracts_all_group_configs() {
        let wc = PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        let configs = wc.configs();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].fixture, "Simple");
        assert_eq!(configs[1].key(), "BackSimple");
    }

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
        assert!(validate_option(&opt, "z").is_ok());
    }

    #[test]
    fn swap_groups_reorders() {
        let mut wc =
            PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        assert_eq!(wc.groups[0].config.key(), "Simple");
        assert_eq!(wc.groups[1].config.key(), "BackSimple");
        wc.groups.swap(0, 1);
        assert_eq!(wc.groups[0].config.key(), "BackSimple");
        assert_eq!(wc.groups[1].config.key(), "Simple");
    }

    fn ua(universe: usize, address: usize) -> UniverseAddress {
        UniverseAddress { universe, address }
    }

    #[test]
    fn address_map_build() {
        let wc = PatchWorkingCopy::from_snapshot(&test_snapshot_with_groups(), &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.0[&ua(0, 1)].len(), 1);
        assert_eq!(map.0[&ua(0, 1)][0], "Simple");
        assert!(map.0.contains_key(&ua(0, 10)));
        assert!(map.0.contains_key(&ua(0, 11)));
    }

    #[test]
    fn address_map_detects_collision() {
        let snapshot = PatchSnapshot {
            groups: vec![simple_group(Some("A"), &[1]), simple_group(Some("B"), &[1])],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.0[&ua(0, 1)].len(), 2);
        let collision = map.collision_at(ua(0, 1));
        assert!(collision.is_some());
        assert!(collision.unwrap().contains("A"));
    }

    #[test]
    fn address_map_no_collision_different_universes() {
        let snapshot = PatchSnapshot {
            groups: vec![
                simple_group(Some("A"), &[1]),
                FixtureGroupConfig {
                    fixture: "Simple".to_string(),
                    group: Some(FixtureGroupKey("B".to_string())),
                    channel: true,
                    color_organ: false,
                    patches: vec![PatchBlock {
                        addr: Some(DmxAddrConfig::Single(DmxAddr::new(1))),
                        universe: 1,
                        mirror: false,
                        options: Options::default(),
                    }],
                    options: Options::default(),
                },
            ],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.0[&ua(0, 1)].len(), 1);
        assert_eq!(map.0[&ua(1, 1)].len(), 1);
    }

    #[test]
    fn address_map_multi_channel_fixture() {
        let snapshot = PatchSnapshot {
            groups: vec![patch_opts_group(None, vec![patch_opts_block(10, "Wide")])],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        for addr in 10..=15 {
            assert!(
                map.0.contains_key(&ua(0, addr)),
                "expected addr {addr} in map"
            );
        }
        assert!(!map.0.contains_key(&ua(0, 16)));
    }

    #[test]
    fn find_available_skips_used_addresses() {
        let snapshot = test_snapshot_with_groups();
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.find_available(0, 1, 1), Some(2));
        assert_eq!(map.find_available(0, 3, 1), Some(2));
        assert_eq!(map.find_available(0, 1, 12), Some(12));
    }

    #[test]
    fn find_available_wraps_around() {
        let snapshot = PatchSnapshot {
            groups: vec![simple_group(None, &(500..=512).collect::<Vec<_>>())],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.find_available(0, 1, 510), Some(1));
    }

    #[test]
    fn find_available_returns_none_when_full() {
        let snapshot = PatchSnapshot {
            groups: vec![simple_group(None, &(1..=512).collect::<Vec<_>>())],
        };
        let wc = PatchWorkingCopy::from_snapshot(&snapshot, &test_patchers());
        let map = AddressMap::from_working_copy(&wc);
        assert_eq!(map.find_available(0, 1, 1), None);
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

    // -----------------------------------------------------------------------
    // Snapshot tests
    // -----------------------------------------------------------------------

    use crate::control::mock::auto_respond_client;
    use crate::ui_util::MessageModal;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn snapshot_panel(
        snapshot: &PatchSnapshot,
        patchers: &[Patcher],
        state: &mut PatchPanelState,
        name: &str,
    ) {
        let client = auto_respond_client();
        let mut modal = MessageModal::default();

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state,
                snapshot,
                patchers,
            }
            .ui(ui);
        });
        harness.run();
        harness.snapshot(name);
    }

    #[test]
    fn render_empty_patch() {
        let mut state = PatchPanelState::new();
        snapshot_panel(
            &test_snapshot_empty(),
            &test_patchers(),
            &mut state,
            "patch_panel_empty",
        );
    }

    #[test]
    fn render_with_groups() {
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);
        snapshot_panel(
            &test_snapshot_with_groups(),
            &test_patchers(),
            &mut state,
            "patch_panel_with_groups",
        );
    }

    #[test]
    fn render_second_group_selected() {
        let mut state = PatchPanelState::new();
        state.selected_group = Some(1);
        snapshot_panel(
            &test_snapshot_with_groups(),
            &test_patchers(),
            &mut state,
            "patch_panel_second_group",
        );
    }

    #[test]
    fn render_add_group_form() {
        let patchers = test_patchers();
        let mut state = PatchPanelState::new();
        let mut form = AddGroupForm::new();
        form.sync_options(&patchers);
        state.mode = PanelMode::AddGroup(form);
        snapshot_panel(
            &test_snapshot_with_groups(),
            &patchers,
            &mut state,
            "patch_panel_add_group",
        );
    }

    #[test]
    fn render_with_patch_options() {
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);
        snapshot_panel(
            &test_snapshot_with_options(),
            &test_patchers(),
            &mut state,
            "patch_panel_patch_options",
        );
    }

    #[test]
    fn render_with_group_options() {
        let mut state = PatchPanelState::new();
        state.selected_group = Some(1);
        snapshot_panel(
            &test_snapshot_with_options(),
            &test_patchers(),
            &mut state,
            "patch_panel_group_options",
        );
    }

    #[test]
    fn render_with_dmx_map() {
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);
        state.show_address_map = true;
        snapshot_panel(
            &test_snapshot_with_groups(),
            &test_patchers(),
            &mut state,
            "patch_panel_dmx_map",
        );
    }

    fn setup_add_fixture(
        snapshot: &PatchSnapshot,
        patchers: &[Patcher],
        group_idx: usize,
        state: &mut PatchPanelState,
    ) {
        state.selected_group = Some(group_idx);
        state.working_copy = Some(PatchWorkingCopy::from_snapshot(snapshot, patchers));
        let wc = state.working_copy.as_ref().unwrap();
        let fixture_type = &wc.groups[group_idx].config.fixture;
        let patcher = patchers
            .iter()
            .find(|p| p.name.0 == *fixture_type)
            .unwrap();
        let addr_map = AddressMap::from_working_copy(wc);
        let form = AddFixtureForm::new_for_group(&wc.groups[group_idx], patcher, &addr_map);
        state.mode = PanelMode::AddFixture(form);
    }

    #[test]
    fn render_add_fixture_simple() {
        let patchers = test_patchers();
        let snapshot = test_snapshot_with_groups();
        let mut state = PatchPanelState::new();
        setup_add_fixture(&snapshot, &patchers, 0, &mut state);
        snapshot_panel(
            &snapshot,
            &patchers,
            &mut state,
            "patch_panel_add_fixture_simple",
        );
    }

    #[test]
    fn render_add_fixture_with_patch_options() {
        let patchers = test_patchers();
        let snapshot = test_snapshot_with_options();
        let mut state = PatchPanelState::new();
        setup_add_fixture(&snapshot, &patchers, 0, &mut state);
        snapshot_panel(
            &snapshot,
            &patchers,
            &mut state,
            "patch_panel_add_fixture_with_opts",
        );
    }

    // -----------------------------------------------------------------------
    // Interaction tests
    // -----------------------------------------------------------------------

    /// Test that clicking "Delete Group" in the detail view triggers the delete modal.
    #[test]
    fn click_delete_in_detail_triggers_modal() {
        let client = auto_respond_client();
        let snapshot = test_snapshot_with_groups();
        let patchers = test_patchers();
        let mut modal = MessageModal::default();
        let mut state = PatchPanelState::new();
        state.selected_group = Some(0);

        let mut harness = Harness::new_ui(|ui| {
            PatchPanel {
                ctx: GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                state: &mut state,
                snapshot: &snapshot,
                patchers: &patchers,
            }
            .ui(ui);
        });
        harness.run();

        // Find and click the "Delete Group" button in the detail view.
        let del_btn = harness.get_by_label("Delete Group");
        del_btn.click();
        harness.run();
        drop(harness);

        assert_eq!(
            state.pending_delete,
            Some(0),
            "Expected pending_delete=Some(0), got {:?}",
            state.pending_delete
        );
    }
}
