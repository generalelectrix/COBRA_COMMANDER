use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;

use crate::config::FixtureGroupConfig;
use crate::fixture::Patch;
use crate::show_file::{self, ShowFile};
use crate::ui_util::MessageModal;

/// The result of the welcome screen interaction.
#[derive(Debug)]
pub(crate) enum WelcomeResult {
    /// User chose to load an existing show file (validated).
    LoadShow {
        path: PathBuf,
        configs: Vec<FixtureGroupConfig>,
    },
    /// User chose to create a new, empty show.
    NewShow { path: PathBuf },
    /// User closed the welcome window without choosing.
    Quit,
}

struct WelcomeApp {
    result: Arc<Mutex<Option<WelcomeResult>>>,
    modal: MessageModal,
}

impl eframe::App for WelcomeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame = egui::Frame::central_panel(&ctx.style());
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new(egui::ImageSource::Bytes {
                    uri: "bytes://emblem".into(),
                    bytes: egui::load::Bytes::Static(include_bytes!("../../resources/splash.png")),
                }));

                ui.heading(egui::RichText::new("COBRA COMMANDER").size(60.0).strong());

                ui.horizontal(|ui| {
                    let button_width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    let button_size = egui::vec2(button_width, 50.0);
                    let button_font = egui::FontId::proportional(20.0);
                    let button_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
                    if ui
                        .add_sized(
                            button_size,
                            egui::Button::new(
                                egui::RichText::new("NEW")
                                    .font(button_font.clone())
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::BLACK)
                            .stroke(button_stroke),
                        )
                        .clicked()
                    {
                        self.handle_new(ctx);
                    }
                    if ui
                        .add_sized(
                            button_size,
                            egui::Button::new(
                                egui::RichText::new("LOAD")
                                    .font(button_font)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::BLACK)
                            .stroke(button_stroke),
                        )
                        .clicked()
                    {
                        self.handle_load(ctx);
                    }
                });
            });
        });

        self.modal.ui(ctx);
    }
}

impl WelcomeApp {
    fn handle_load(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(show_file::FILTER_NAME, &[show_file::EXTENSION])
            .pick_file()
        else {
            return;
        };

        let show_file = match show_file::load(&path) {
            Ok(f) => f,
            Err(e) => {
                self.modal.show("Failed to Load", format!("{e:#}"));
                return;
            }
        };

        // Validate the patch by building it (discards the result — Patch isn't Send).
        if let Err(e) = Patch::patch_all(&show_file.patch) {
            self.modal.show("Invalid Show File", format!("{e:#}"));
            return;
        }

        *self.result.lock().expect("welcome result mutex poisoned") =
            Some(WelcomeResult::LoadShow {
                path,
                configs: show_file.patch,
            });
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn handle_new(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(show_file::FILTER_NAME, &[show_file::EXTENSION])
            .set_file_name(show_file::DEFAULT_FILE_NAME)
            .save_file()
        else {
            return;
        };

        let empty = ShowFile { patch: vec![] };
        if let Err(e) = show_file::save(&path, &empty) {
            self.modal.show("Failed to Create Show", format!("{e:#}"));
            return;
        }

        *self.result.lock().expect("welcome result mutex poisoned") =
            Some(WelcomeResult::NewShow { path });
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Run the welcome screen. Returns the user's choice.
pub(crate) fn run_welcome() -> Result<WelcomeResult> {
    let result: Arc<Mutex<Option<WelcomeResult>>> = Arc::new(Mutex::new(None));
    let app_result = result.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([624.0, 634.0])
            .with_resizable(false)
            .with_icon(std::sync::Arc::new(egui::IconData::default())),
        ..Default::default()
    };

    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |cc| {
            super::apply_dark_theme(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(WelcomeApp {
                result: app_result,
                modal: MessageModal::default(),
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe welcome window failed: {e}"))?;

    let inner = Arc::try_unwrap(result)
        .map_err(|_| anyhow::anyhow!("welcome result Arc still shared"))?
        .into_inner()
        .expect("welcome result mutex poisoned");
    Ok(inner.unwrap_or(WelcomeResult::Quit))
}
