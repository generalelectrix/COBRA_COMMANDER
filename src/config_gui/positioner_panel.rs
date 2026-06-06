//! GUI tab for renaming positioner preset slots.
//!
//! The operator's flow:
//!
//! 1. On TouchOSC, select the channel + preset slot they want to name.
//! 2. On the desktop, type the name and press Enter (or click Apply).
//!
//! This panel is intentionally a dumb write-only text box. It does not
//! reflect show state — it knows nothing about what's currently selected.
//! On Enter with non-empty trimmed input, it fires a
//! [`MetaCommand::RenamePositionerPreset`] and clears its own buffer. The
//! show resolves "which preset" from the currently-selected channel's
//! group's active slot at command-handling time. If the current channel
//! isn't positionable (or there's no current channel), the command is a
//! silent no-op on the show side; the box still clears on this side, so
//! the operator can immediately type the next name.

use eframe::egui;

use crate::control::MetaCommand;
use crate::ui_util::GuiContext;

#[derive(Default)]
pub(crate) struct PositionerPanelState {
    /// Current text in the input box. Always trimmed before being sent.
    input: String,
}

pub(crate) struct PositionerPanel<'a> {
    pub ctx: GuiContext<'a>,
    pub state: &'a mut PositionerPanelState,
}

impl PositionerPanel<'_> {
    pub fn ui(mut self, ui: &mut egui::Ui) {
        ui.heading("Positioner");
        ui.separator();

        ui.label(
            "Type a name for the currently-selected preset slot on TouchOSC, then press \
             Enter (or click Apply).",
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.state.input)
                    .desired_width(240.0)
                    .hint_text("Preset name"),
            );
            let enter_pressed =
                response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let apply_clicked = ui.button("Apply").clicked();

            if enter_pressed || apply_clicked {
                let trimmed = self.state.input.trim().to_string();
                if !trimmed.is_empty() {
                    let _ = self
                        .ctx
                        .send_command(MetaCommand::RenamePositionerPreset(trimmed));
                }
                self.state.input.clear();
                if enter_pressed {
                    // Keep focus in the input so the operator can type the
                    // next name without re-clicking the field.
                    response.request_focus();
                }
            }
        });
    }
}
