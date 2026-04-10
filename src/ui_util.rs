use eframe::egui::{self, Color32};

use crate::control::{CommandClient, MetaCommand};

/// Semantic status colors for consistent theming across panels.
pub struct StatusColors {
    /// Neutral/unassigned state.
    pub inactive: Color32,
    /// Connected/running state.
    pub active: Color32,
    /// Degraded/attention-needed state.
    pub warning: Color32,
    /// Disconnected/failed state.
    pub error: Color32,
    /// Inline validation error text.
    pub error_text: Color32,
    /// Fill color for confirm/accept/apply buttons.
    pub confirm_button: Color32,
    /// Fill color for cancel/revert/dismiss buttons.
    pub cancel_button: Color32,
}

pub const STATUS_COLORS: StatusColors = StatusColors {
    inactive: Color32::GRAY,
    active: Color32::GREEN,
    warning: Color32::from_rgb(255, 165, 0),
    error: Color32::from_rgb(255, 80, 80),
    error_text: Color32::RED,
    confirm_button: Color32::from_rgb(30, 100, 50),
    cancel_button: Color32::from_rgb(80, 80, 80),
};

/// A confirm/accept/apply button with semantic styling.
pub fn confirm_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).fill(STATUS_COLORS.confirm_button))
        .clicked()
}

/// A confirm button that can be disabled.
pub fn confirm_button_enabled(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(text).fill(STATUS_COLORS.confirm_button),
    )
    .clicked()
}

/// A cancel/revert/dismiss button with semantic styling.
pub fn cancel_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).fill(STATUS_COLORS.cancel_button))
        .clicked()
}

// ---------------------------------------------------------------------------
// Layout helpers — font-scaled sizing for scroll areas and text fields.
//
// These compute dimensions relative to the current font and spacing so that
// layouts adapt automatically when the font or theme changes.
// ---------------------------------------------------------------------------

/// Compute the height needed for `rows` table rows, scaled to the current font and spacing.
///
/// Useful for setting `ScrollArea::max_height` or `min_scrolled_height` so that
/// a table shows a predictable number of rows regardless of font size.
pub fn row_height_for(ui: &egui::Ui, rows: usize) -> f32 {
    let row_height = ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;
    row_height * rows as f32
}

/// Compute the width needed for `chars` characters of text, scaled to the current font.
///
/// Useful for setting `TextEdit::desired_width` on numeric fields so they
/// don't stretch to fill available space. Includes padding for the text
/// edit frame.
pub fn char_width_for(ui: &egui::Ui, chars: usize) -> f32 {
    let char_width = ui.text_style_height(&egui::TextStyle::Body) * 0.6;
    char_width * chars as f32 + ui.spacing().button_padding.x * 2.0
}

/// Shared rendering context for GUI panels.
///
/// Bundles the dependencies common to all panel renderers so they don't
/// need to be threaded through every method call.
pub(crate) struct GuiContext<'a> {
    pub modal: &'a mut MessageModal,
    pub client: &'a CommandClient,
}

impl GuiContext<'_> {
    pub fn report_error(&mut self, error: impl std::fmt::Display) {
        self.modal.show("Error", error.to_string());
    }

    pub fn send_command(&mut self, cmd: MetaCommand) -> Result<(), anyhow::Error> {
        self.client.send_command(cmd).inspect_err(|e| {
            self.modal.show("Error", e.to_string());
        })
    }
}

/// Result of a drag-and-drop reorder interaction on a single row.
pub struct DndReorderResult {
    /// If `Some`, this row received a drop — the caller should swap source and target.
    pub swap: Option<(usize, usize)>,
}

/// Apply drag-and-drop reorder behavior to a response.
///
/// Handles cursor feedback (grab/grabbing icons), drag payload management,
/// and painting a drop-indicator line. Call this once per row in a reorderable
/// list, then collect the `swap` results to apply after the loop.
pub fn dnd_reorder(
    ui: &egui::Ui,
    response: &egui::Response,
    row_index: usize,
    indicator_x_range: impl Into<egui::Rangef>,
) -> DndReorderResult {
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    response.dnd_set_drag_payload(row_index);

    let mut swap = None;
    if let Some(source_idx) = response.dnd_release_payload::<usize>() {
        swap = Some((*source_idx, row_index));
    }

    if let Some(source_idx) = response.dnd_hover_payload::<usize>() {
        let selection_color = ui.style().visuals.selection.bg_fill;
        let y = if *source_idx <= row_index {
            response.rect.bottom()
        } else {
            response.rect.top()
        };
        ui.painter().hline(
            indicator_x_range,
            y,
            egui::Stroke::new(2.0, selection_color),
        );
    }

    DndReorderResult { swap }
}

/// Displays a modal dialog with a title and message, blocked until dismissed.
#[derive(Default)]
pub struct MessageModal {
    pending: Option<(String, String)>,
}

impl MessageModal {
    pub fn show(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.pending = Some((title.into(), message.into()));
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some((title, message)) = &self.pending else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("message_modal")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading(title.as_str());
            ui.label(message.as_str());
            ui.add_space(8.0);
            if ui.button("OK").clicked() {
                ui.close();
            }
        });
        if response.should_close() {
            self.pending = None;
        }
    }
}

/// Handles window close confirmation for egui apps.
///
/// Intercepts the viewport close request, shows a confirmation dialog,
/// and only allows closing when the user confirms.
#[derive(Default)]
pub struct CloseHandler {
    show_confirmation_dialog: bool,
    allowed_to_close: bool,
}

impl CloseHandler {
    pub fn update(&mut self, quit_prompt: &str, ctx: &egui::Context) {
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
