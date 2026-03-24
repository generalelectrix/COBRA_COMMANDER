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
    ui.add_enabled(enabled, egui::Button::new(text).fill(STATUS_COLORS.confirm_button))
        .clicked()
}

/// A cancel/revert/dismiss button with semantic styling.
pub fn cancel_button(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).fill(STATUS_COLORS.cancel_button))
        .clicked()
}

/// Shared rendering context for GUI panels.
///
/// Bundles the dependencies common to all panel renderers so they don't
/// need to be threaded through every method call.
pub(crate) struct GuiContext<'a> {
    pub error_modal: &'a mut ErrorModal,
    pub client: &'a CommandClient,
}

impl GuiContext<'_> {
    pub fn report_error(&mut self, error: impl std::fmt::Display) {
        self.error_modal.show(error.to_string());
    }

    pub fn send_command(&mut self, cmd: MetaCommand) -> Result<(), anyhow::Error> {
        self.client.send_command(cmd).inspect_err(|e| {
            self.error_modal.show(e.to_string());
        })
    }
}

/// Displays a modal error dialog that blocks interaction until dismissed.
#[derive(Default)]
pub struct ErrorModal {
    message: Option<String>,
}

impl ErrorModal {
    pub fn show(&mut self, error: String) {
        self.message = Some(error);
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        let Some(message) = &self.message else { return };
        let modal_response = egui::Modal::new(egui::Id::new("error_modal")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading("Error");
            ui.label(message.as_str());
            ui.add_space(8.0);
            if ui.button("OK").clicked() {
                ui.close();
            }
        });
        if modal_response.should_close() {
            self.message = None;
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
