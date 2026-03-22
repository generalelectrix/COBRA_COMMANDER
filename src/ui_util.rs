use eframe::egui;

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
