use eframe::egui;

use crate::control::{CommandClient, MetaCommand};
use gui_common::MessageModal;

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

/// Default number of table rows a bounded list shows before it scrolls.
pub const SCROLL_MAX_ROWS: usize = 8;

/// Wrap `content` in a vertical scroll area that grows with its content up to
/// `max_rows` rows, then scrolls. The scrollbar stays hidden until the content
/// exceeds that height.
pub fn bounded_scroll<R>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    max_rows: usize,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::ScrollArea::vertical()
        .max_height(row_height_for(ui, max_rows))
        .auto_shrink([false, true]) // keep full width; shrink height to content
        .id_salt(id_salt)
        .show(ui, content)
        .inner
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

    pub fn report_info(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.modal.show(title, message);
    }

    pub fn send_command(&mut self, cmd: MetaCommand) -> Result<(), anyhow::Error> {
        self.client.send_command(cmd).inspect_err(|e| {
            self.modal.show("Error", e.to_string());
        })
    }
}
