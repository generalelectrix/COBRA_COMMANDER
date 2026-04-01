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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::fixture::Patch;
use crate::gui_state::SharedGuiState;
use crate::ui_util::{CloseHandler, GuiContext, MessageModal};
use animation_panel::VisualizerPanelState;
use clock_panel::{ClockPanel, ClockPanelState};
use dmx_panel::{DmxPortPanel, DmxPortPanelState};
use midi_panel::{MidiPanel, MidiPanelState};
use patch_panel::{PatchPanel, PatchPanelState};

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

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanelState,
    midi_panel: MidiPanelState,
    /// Behind Arc<Mutex<>> because this state is shared between the embedded
    /// Animation tab and the detached viewport (which runs on a separate
    /// thread via show_viewport_deferred). Only one renders at a time, so the
    /// mutex is never contended.
    visualizer_panel: Arc<Mutex<VisualizerPanelState>>,
    /// Shared with the deferred viewport closure so it can signal "close" back
    /// to the main thread. Arc<AtomicBool> because the deferred closure is
    /// 'static + Send + Sync and can't hold a reference to ConfigApp fields.
    visualizer_detached: Arc<AtomicBool>,
    patch_panel: PatchPanelState,
    dmx_panel: DmxPortPanelState,
    patchers: Vec<crate::fixture::patch::Patcher>,
    close_handler: CloseHandler,
    modal: MessageModal,
    active_tab: Tab,
    gui_state: SharedGuiState,
}

impl eframe::App for ConfigApp {
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
                let mut ctx = GuiContext {
                    modal: &mut self.modal,
                    client: &self.client,
                };
                osc_panel::ui(ui, &mut ctx, &self.gui_state.osc_listen_addr, &clients);
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

pub fn run_config_gui(
    client: CommandClient,
    zmq_ctx: zmq::Context,
    gui_state: SharedGuiState,
) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([600.0, 500.0]),
        ..Default::default()
    };
    let initial_clock_status = gui_state.clock_status.load();
    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ConfigApp {
                clock_panel: ClockPanelState::new(zmq_ctx, &initial_clock_status),
                midi_panel: MidiPanelState::new(),
                visualizer_panel: Arc::new(Mutex::new(VisualizerPanelState::default())),
                visualizer_detached: Arc::new(AtomicBool::new(false)),
                patch_panel: PatchPanelState::new(),
                dmx_panel: DmxPortPanelState::new(),
                patchers: Patch::menu(),
                client,
                close_handler: CloseHandler::default(),
                modal: MessageModal::default(),
                active_tab: Tab::default(),
                gui_state,
            }))
        }),
    )
    .unwrap();
    Ok(())
}
