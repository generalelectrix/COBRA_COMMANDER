//! GUI configuration panels for Cobra Commander.
//!
//! # Panel patterns
//!
//! Panels follow a complexity gradient:
//!
//! - **Read-only**: bare function `fn ui(ui, &Data)`
//! - **Read + commands**: function taking `GuiContext`
//! - **Stateful + commands**: `FooPanelState` + `FooPanel<'a>` render wrapper
//!
//! # State flow
//!
//! - Show → GUI: `ArcSwap` fields on `SharedGuiState` (lock-free reads)
//! - GUI → Show: `MetaCommand` via `GuiContext::send_command()` (blocking with error modal)
//! - Panel-local UI state (combo box selections, etc.) lives in `FooPanelState`
//!   and syncs from the authoritative Show state via `sync_from_status()`

mod animation_panel;
mod clock_panel;
mod dmx_panel;
mod midi_panel;
mod osc_panel;
mod patch_panel;
mod welcome;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;
use local_ip_address::local_ip;
use log::error;
use midi_harness::install_midi_device_change_handler;

use crate::clocks::Clocks;
use crate::control::{CommandClient, Controller};
use crate::fixture::Patch;
use crate::gui_state::{ClockStatus, GuiState, SharedGuiState};
use crate::midi::ControlHandler;
use crate::preview::Previewer;
use crate::show::Show;
use crate::ui_util::{CloseHandler, GuiContext, MessageModal};
use animation_panel::VisualizerPanelState;
use clock_panel::{ClockPanel, ClockPanelState};
use dmx_panel::{DmxPortPanel, DmxPortPanelState};
use midi_panel::{MidiPanel, MidiPanelState};
use patch_panel::{PatchPanel, PatchPanelState};
use welcome::WelcomeResult;

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Midi,
    Osc,
    Clocks,
    Animation,
    Patch,
    Dmx,
}

struct ConsoleApp {
    client: CommandClient,
    show_file_path: PathBuf,
    clock_panel: ClockPanelState,
    midi_panel: MidiPanelState,
    /// Behind Arc<Mutex<>> because this state is shared between the embedded
    /// Animation tab and the detached viewport (which runs on a separate
    /// thread via show_viewport_deferred). Only one renders at a time, so the
    /// mutex is never contended.
    visualizer_panel: Arc<Mutex<VisualizerPanelState>>,
    /// Shared with the deferred viewport closure so it can signal "close" back
    /// to the main thread. Arc<AtomicBool> because the deferred closure is
    /// 'static + Send + Sync and can't hold a reference to ConsoleApp fields.
    visualizer_detached: Arc<AtomicBool>,
    osc_panel: osc_panel::OscPanelState,
    patch_panel: PatchPanelState,
    dmx_panel: DmxPortPanelState,
    patchers: Vec<crate::fixture::patch::Patcher>,
    close_handler: CloseHandler,
    modal: MessageModal,
    active_tab: Tab,
    gui_state: SharedGuiState,
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler.update("Quit Cobra Commander?", ctx);

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Midi, "MIDI");
                ui.selectable_value(&mut self.active_tab, Tab::Osc, "OSC");
                ui.selectable_value(&mut self.active_tab, Tab::Clocks, "Clocks");
                ui.selectable_value(&mut self.active_tab, Tab::Animation, "Animation");
                ui.selectable_value(&mut self.active_tab, Tab::Patch, "Patch");
                ui.selectable_value(&mut self.active_tab, Tab::Dmx, "DMX");
            });
        });

        // Notify the show when the visualizer is visible (either tab or detached window).
        let detached = self.visualizer_detached.load(Ordering::Relaxed);
        self.gui_state.visualizer_active.store(
            detached || self.active_tab == Tab::Animation,
            Ordering::Relaxed,
        );

        // Detached animation visualizer — separate OS window via deferred viewport.
        if detached {
            let gui_state = self.gui_state.clone();
            let detached_flag = self.visualizer_detached.clone();
            let panel = self.visualizer_panel.clone();
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of("animation_visualizer"),
                egui::ViewportBuilder::default()
                    .with_title("Animation Visualizer")
                    .with_inner_size(egui::vec2(600.0, 400.0)),
                move |ctx, _class| {
                    let Ok(mut panel) = panel.lock() else { return };
                    let snapshot = gui_state.animation_state.load();
                    egui::CentralPanel::default().show(ctx, |ui| {
                        panel.ui(ui, &snapshot);
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        detached_flag.store(false, Ordering::Relaxed);
                    }
                },
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Clocks => {
                let clock_status = self.gui_state.clock_status.load();
                ClockPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.clock_panel,
                    clock_status: &clock_status,
                }
                .ui(ui);
            }
            Tab::Midi => {
                let midi_slots = self.gui_state.midi_slots.load();
                let master_strobe = self
                    .gui_state
                    .master_strobe_fader_channel_mapped
                    .load(std::sync::atomic::Ordering::Relaxed);
                MidiPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.midi_panel,
                    slots: &midi_slots,
                    master_strobe_fader_channel_mapped: master_strobe,
                }
                .ui(ui);
            }
            Tab::Osc => {
                let clients = self.gui_state.osc_clients.load();
                let patch_snapshot = self.gui_state.patch_snapshot.load();
                osc_panel::OscPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.osc_panel,
                    listen_addr: &self.gui_state.osc_listen_addr,
                    clients: &clients,
                    groups: &patch_snapshot.groups,
                    show_file_path: &self.show_file_path,
                }
                .ui(ui);
            }
            Tab::Animation => {
                if self.visualizer_detached.load(Ordering::Relaxed) {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.label("Visualizer is in a separate window.");
                        if ui.button("Reattach").clicked() {
                            self.visualizer_detached.store(false, Ordering::Relaxed);
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Detach").clicked() {
                                self.visualizer_detached.store(true, Ordering::Relaxed);
                            }
                        });
                    });
                    let snapshot = self.gui_state.animation_state.load();
                    if let Ok(mut panel) = self.visualizer_panel.lock() {
                        panel.ui(ui, &snapshot);
                    }
                }
            }
            Tab::Patch => {
                let snapshot = self.gui_state.patch_snapshot.load();
                PatchPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.patch_panel,
                    snapshot: &snapshot,
                    patchers: &self.patchers,
                    show_file_path: &self.show_file_path,
                }
                .ui(ui);
            }
            Tab::Dmx => {
                let port_status = self.gui_state.dmx_port_status.load();
                DmxPortPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.dmx_panel,
                    port_status: &port_status,
                }
                .ui(ui);
            }
        });

        self.modal.ui(ctx);
    }
}

/// Single entry point for the console. Runs the welcome screen, initializes
/// the show, and runs the main console GUI.
pub fn run_console(osc_receive_port: u16) -> Result<()> {
    // Phase 1: Welcome screen.
    let welcome_result = welcome::run_welcome()?;

    let (show_file_path, initial_configs) = match welcome_result {
        WelcomeResult::LoadShow { path, configs } => (path, configs),
        WelcomeResult::NewShow { path } => (path, vec![]),
        WelcomeResult::Quit => return Ok(()),
    };

    // Phase 2: Create all infrastructure.
    let osc_listen_addr = match local_ip() {
        Ok(ip) => format!("{ip}:{osc_receive_port}"),
        Err(_) => format!("0.0.0.0:{osc_receive_port}"),
    };

    let zmq_ctx = zmq::Context::new();
    let (send_control_msg, recv_control_msg) = channel();
    let command_client = CommandClient::new(send_control_msg.clone());

    // NOTE: this MUST be called before any other MIDI functions.
    install_midi_device_change_handler(ControlHandler(send_control_msg.clone()))?;

    let controller = Controller::new(
        osc_receive_port,
        vec![],
        vec![],
        send_control_msg,
        recv_control_msg,
    )?;
    let osc_client_listener = controller.osc_client_listener();

    let gui_state: SharedGuiState = Arc::new(GuiState::new(
        vec![],
        ClockStatus::Internal {
            audio_device: "Offline".into(),
        },
        osc_listen_addr,
        osc_client_listener,
    ));

    // Phase 3: Spawn the show thread.
    // Patch is created on the show thread because it contains non-Send types.
    let show_gui_state = gui_state.clone();
    std::thread::spawn(move || {
        let patch = match Patch::patch_all(&initial_configs) {
            Ok(p) => p,
            Err(e) => {
                error!("Show patch error: {e:#}");
                return;
            }
        };
        let universe_count = patch.universe_count();
        let dmx = (0..universe_count)
            .map(|_| crate::dmx::DmxUniverse::offline())
            .collect();
        let clocks = Clocks::internal(None);
        let show = Show::new(
            patch,
            initial_configs,
            controller,
            dmx,
            clocks,
            Previewer::default(),
            show_gui_state,
        );
        match show {
            Ok(mut show) => show.run(),
            Err(e) => error!("Show initialization error: {e:#}"),
        }
    });

    // Phase 4: Run the console GUI.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 500.0])
            .with_icon(std::sync::Arc::new(egui::IconData::default())),
        ..Default::default()
    };
    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ConsoleApp {
                clock_panel: ClockPanelState::new(
                    zmq_ctx,
                    &ClockStatus::Internal {
                        audio_device: "Offline".into(),
                    },
                ),
                midi_panel: MidiPanelState::new(),
                visualizer_panel: Arc::new(Mutex::new(VisualizerPanelState::default())),
                visualizer_detached: Arc::new(AtomicBool::new(false)),
                osc_panel: osc_panel::OscPanelState::new(),
                patch_panel: PatchPanelState::new(),
                dmx_panel: DmxPortPanelState::new(),
                patchers: Patch::menu(),
                client: command_client,
                show_file_path,
                close_handler: CloseHandler::default(),
                modal: MessageModal::default(),
                active_tab: Tab::default(),
                gui_state,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe console window failed: {e}"))?;
    Ok(())
}
