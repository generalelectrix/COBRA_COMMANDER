//! DMX output debug window.
//!
//! Pops out (via `show_viewport_deferred`) from the DMX tab and displays the
//! live DMX output buffer for a selected universe as a heat-mapped grid of the
//! 512 channel values. The Show only pushes data while this window is open and
//! only for the selected universe (see [`crate::gui_state::dmx_debug_watch`]),
//! throttled to ~4fps, so this view is cheap for the real-time thread.

use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui;

use crate::dmx::DmxBuffer;
use crate::gui_state::SharedGuiState;

/// Render the DMX output debug view.
///
/// `selected_universe` is the shared selection the main loop reads to tell the
/// Show which universe to snapshot; the combo box here writes to it.
pub(crate) fn dmx_debug_panel_ui(
    ui: &mut egui::Ui,
    gui_state: &SharedGuiState,
    selected_universe: &AtomicUsize,
) {
    // The global stage theme inflates spacing and font sizes for at-a-distance
    // legibility, which makes this dense 512-cell grid require an enormous
    // window. Restore egui's default spacing and font sizes for this window's
    // UI subtree only — keeping the dark theme colors — so the values fit.
    // `style_mut` is copy-on-write, so this does not touch the shared context.
    {
        let defaults = egui::Style::default();
        let style = ui.style_mut();
        style.spacing = defaults.spacing;
        style.text_styles = defaults.text_styles;
        // Labels are selectable by default, which gives each grid cell a text
        // I-beam cursor and swallows the hover tooltip. These cells are
        // read-only numbers, so disable selection — restoring the normal cursor
        // and letting the per-cell channel/value tooltip show.
        style.interaction.selectable_labels = false;
    }

    let universe_count = gui_state.dmx_port_status.load().ports.len();
    if universe_count == 0 {
        ui.label("No universes patched.");
        return;
    }

    // Clamp the selection in case a repatch shrank the universe count.
    let mut selected = selected_universe
        .load(Ordering::Relaxed)
        .min(universe_count - 1);

    ui.horizontal(|ui| {
        // `from_id_salt` (not `from_label`) so egui doesn't render a redundant
        // "Universe" label next to the box — the selected text already says it.
        egui::ComboBox::from_id_salt("dmx_debug_universe")
            .selected_text(format!("Universe {selected}"))
            .show_ui(ui, |ui| {
                for i in 0..universe_count {
                    ui.selectable_value(&mut selected, i, format!("Universe {i}"));
                }
            });
    });
    selected_universe.store(selected, Ordering::Relaxed);

    ui.separator();

    // Only show data tagged with the universe we currently have selected; a
    // stale snapshot from the previous selection is dropped until the Show
    // catches up (≤ one snapshot interval).
    let snapshot = gui_state.dmx_debug.load();
    match &**snapshot {
        Some(snap) if snap.universe == selected => render_grid(ui, &snap.values),
        _ => {
            ui.add_space(8.0);
            ui.label(format!("Waiting for universe {selected}…"));
        }
    }
}

/// Render the 512 channel values as a 16-wide, 32-row grid. Each cell shows the
/// decimal value with a background tinted dark→amber by intensity so lit
/// channels stand out at a glance. The left gutter shows the 1-indexed starting
/// channel of each row and the header numbers the columns 1–16; hovering a cell
/// shows its exact 1-indexed channel and value (DMX has no channel 0).
fn render_grid(ui: &mut egui::Ui, values: &DmxBuffer) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("dmx_debug_grid")
            .spacing(egui::vec2(4.0, 2.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Ch").monospace().weak());
                for col in 0..16 {
                    ui.label(
                        egui::RichText::new(format!("{:>3}", col + 1))
                            .monospace()
                            .weak(),
                    );
                }
                ui.end_row();

                for row in 0..32 {
                    let start = row * 16 + 1; // 1-indexed channel number
                    ui.label(
                        egui::RichText::new(format!("{start:>3}"))
                            .monospace()
                            .weak(),
                    );
                    for col in 0..16 {
                        let channel = row * 16 + col + 1; // 1-indexed DMX channel
                        let value = values.get(row * 16 + col).copied().unwrap_or(0);
                        let response = ui.label(
                            egui::RichText::new(format!("{value:>3}"))
                                .monospace()
                                .background_color(heat_color(value))
                                .color(text_color(value)),
                        );
                        // Show the channel/value immediately on rollover rather
                        // than via `on_hover_text`, which waits out the shared
                        // `tooltip_delay` (and we can't shorten that per-window
                        // since tooltip timing is read from the global context).
                        if response.contains_pointer() {
                            response.show_tooltip_text(format!("Channel {channel}: {value}"));
                        }
                    }
                    ui.end_row();
                }
            });
    });
}

/// Background tint for a channel value: black at 0, amber at full.
fn heat_color(value: u8) -> egui::Color32 {
    let t = value as f32 / 255.0;
    egui::Color32::from_rgb((255.0 * t) as u8, (176.0 * t) as u8, 0)
}

/// Readable text color over [`heat_color`]: dark over bright amber, light otherwise.
fn text_color(value: u8) -> egui::Color32 {
    if value > 140 {
        egui::Color32::BLACK
    } else {
        egui::Color32::from_gray(210)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;

    use egui_kittest::{Harness, kittest::Queryable};
    use tunnels_lib::repaint::noop_repaint;

    use crate::dmx::DmxBuffer;
    use crate::gui_state::{ClockStatus, DmxDebugSnapshot, DmxPortInfo, DmxPortStatus, GuiState};

    /// Shared GUI state with `universes` offline ports and an optional pushed
    /// debug snapshot.
    fn gui_state(universes: usize, snapshot: Option<DmxDebugSnapshot>) -> SharedGuiState {
        let state = Arc::new(GuiState::new(
            vec![],
            ClockStatus::Internal {
                audio_device: String::new(),
            },
            String::new(),
            0,
            noop_repaint(),
            noop_repaint(),
        ));
        state.dmx_port_status.store(Arc::new(DmxPortStatus {
            ports: (0..universes)
                .map(|_| DmxPortInfo {
                    name: "offline".to_string(),
                    framerate: None,
                })
                .collect(),
        }));
        state.dmx_debug.store(snapshot);
        state
    }

    /// A ramp across all 512 channels so the heat-map gradient is visible.
    fn ramp_values() -> DmxBuffer {
        let mut values = [0u8; 512];
        for (i, channel) in values.iter_mut().enumerate() {
            *channel = (i % 256) as u8;
        }
        values
    }

    fn snapshot_render(name: &str, gui_state: &SharedGuiState, selected: usize) {
        let selected = AtomicUsize::new(selected);
        // Render at 2x DPI so the dense numeric grid is legible in the snapshot.
        let mut harness = Harness::builder()
            .with_pixels_per_point(2.0)
            .build_ui(|ui| {
                dmx_debug_panel_ui(ui, gui_state, &selected);
            });
        harness.run();
        harness.snapshot(name);
    }

    #[test]
    fn render_grid() {
        let state = gui_state(
            2,
            Some(DmxDebugSnapshot {
                universe: 0,
                values: ramp_values(),
            }),
        );
        snapshot_render("dmx_debug_grid", &state, 0);
    }

    #[test]
    fn render_waiting_for_snapshot() {
        // Universes exist but no snapshot for the selection has arrived yet.
        let state = gui_state(2, None);
        snapshot_render("dmx_debug_waiting", &state, 0);
    }

    #[test]
    fn render_no_universes() {
        let state = gui_state(0, None);
        snapshot_render("dmx_debug_no_universes", &state, 0);
    }

    #[test]
    fn cell_hover_shows_channel_tooltip() {
        // Unique value at channel 1 so we can locate exactly that cell by text.
        let mut values = [0u8; 512];
        values[0] = 200;
        let state = gui_state(
            1,
            Some(DmxDebugSnapshot {
                universe: 0,
                values,
            }),
        );
        let selected = AtomicUsize::new(0);

        // No `tooltip_delay` override: the panel shows the tooltip immediately
        // on rollover, so it must appear on the frame the pointer arrives.
        let mut harness = Harness::new_ui(|ui| {
            dmx_debug_panel_ui(ui, &state, &selected);
        });
        harness.run();

        // The cell for channel 1 renders "200"; hover it.
        let cells: Vec<_> = harness.get_all_by_label("200").collect();
        cells
            .first()
            .unwrap_or_else(|| panic!("no grid cell labeled \"200\" found"))
            .hover();
        harness.run();

        assert!(
            harness.query_by_label("Channel 1: 200").is_some(),
            "expected an immediate \"Channel 1: 200\" tooltip after hovering channel 1's cell",
        );
    }
}
