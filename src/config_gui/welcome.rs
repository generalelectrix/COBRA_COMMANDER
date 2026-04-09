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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading(egui::RichText::new("COBRA COMMANDER").size(32.0).strong());
                ui.add_space(40.0);

                if ui
                    .add_sized([200.0, 40.0], egui::Button::new("Load Show"))
                    .clicked()
                {
                    self.handle_load(ctx);
                }

                ui.add_space(12.0);

                if ui
                    .add_sized([200.0, 40.0], egui::Button::new("New Show"))
                    .clicked()
                {
                    self.handle_new(ctx);
                }
            });
        });

        self.modal.ui(ctx);
    }
}

impl WelcomeApp {
    fn handle_load(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Cobra Show", &["cobra"])
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
            .add_filter("Cobra Show", &["cobra"])
            .set_file_name("show.cobra")
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
            .with_inner_size([400.0, 300.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |_cc| {
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
