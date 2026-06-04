use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;

use crate::config::FixtureGroupConfig;
use crate::fixture::Patch;
use crate::osc;
use crate::show_file::{self, ShowFile};
use gui_common::MessageModal;

/// An OSC receive socket and the port it listens on.
#[derive(Debug)]
pub(crate) struct BoundOsc {
    pub socket: UdpSocket,
    pub port: u16,
}

/// The result of the welcome screen interaction.
#[derive(Debug)]
pub(crate) enum WelcomeResult {
    /// User chose to load an existing show file (validated).
    LoadShow {
        path: PathBuf,
        configs: Vec<FixtureGroupConfig>,
        bound: BoundOsc,
    },
    /// User chose to create a new, empty show.
    NewShow { path: PathBuf, bound: BoundOsc },
    /// User closed the welcome window without choosing.
    Quit,
}

/// A validated show selection held while the OSC port prompt is open.
struct PendingShow {
    path: PathBuf,
    /// Fixture configs for a loaded show; empty for a new show.
    configs: Vec<FixtureGroupConfig>,
    /// Whether this selection creates a new, empty show.
    new: bool,
}

struct WelcomeApp {
    result: Arc<Mutex<Option<WelcomeResult>>>,
    modal: MessageModal,
    /// Port to bind the OSC receive socket to.
    port: u16,
    /// A validated show awaiting a successful OSC port bind.
    pending: Option<PendingShow>,
    /// The bind failure message shown in the port prompt.
    bind_error: String,
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

        if self.pending.is_some() {
            let mut retry = false;
            let mut cancel = false;
            egui::Modal::new(egui::Id::new("osc_port_prompt")).show(ctx, |ui| {
                ui.set_width(360.0);
                ui.heading("OSC Port Unavailable");
                ui.add_space(4.0);
                ui.label(&self.bind_error);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Receive port:");
                    ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    retry = ui.button("Retry").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
            if retry {
                self.attempt_bind(ctx);
            } else if cancel {
                self.pending = None;
                self.bind_error.clear();
            }
        }
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

        self.choose(
            ctx,
            PendingShow {
                path,
                configs: show_file.patch,
                new: false,
            },
        );
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

        self.choose(
            ctx,
            PendingShow {
                path,
                configs: vec![],
                new: true,
            },
        );
    }

    /// Record a chosen show and attempt to bind the OSC port, finalizing on
    /// success or opening the port prompt on failure.
    fn choose(&mut self, ctx: &egui::Context, pending: PendingShow) {
        self.pending = Some(pending);
        self.attempt_bind(ctx);
    }

    /// Bind the OSC receive socket on the configured port. On success, finalize
    /// the result and close the window; on failure, record the error for the
    /// port prompt.
    fn attempt_bind(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        match osc::try_bind(self.port) {
            Ok(socket) => {
                let Some(pending) = self.pending.take() else {
                    return;
                };
                let bound = BoundOsc {
                    socket,
                    port: self.port,
                };
                let result = if pending.new {
                    WelcomeResult::NewShow {
                        path: pending.path,
                        bound,
                    }
                } else {
                    WelcomeResult::LoadShow {
                        path: pending.path,
                        configs: pending.configs,
                        bound,
                    }
                };
                *self.result.lock().expect("welcome result mutex poisoned") = Some(result);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                self.bind_error = format!("{e:#}");
            }
        }
    }
}

/// Run the welcome screen, attempting to bind OSC input on `initial_port` once
/// a show is chosen. Returns the user's choice with the bound socket.
pub(crate) fn run_welcome(initial_port: u16) -> Result<WelcomeResult> {
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
                port: initial_port,
                pending: None,
                bind_error: String::new(),
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
