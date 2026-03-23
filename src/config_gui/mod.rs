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
mod midi_panel;
mod osc_panel;

use std::sync::atomic::Ordering;

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::gui_state::SharedGuiState;
use crate::ui_util::{CloseHandler, ErrorModal, GuiContext, StatusColors};
use animation_panel::VisualizerPanelState;
use clock_panel::{ClockPanel, ClockPanelState};
use midi_panel::{MidiPanel, MidiPanelState};

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Clocks,
    Midi,
    Osc,
    Animation,
}

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanelState,
    midi_panel: MidiPanelState,
    visualizer_panel: VisualizerPanelState,
    close_handler: CloseHandler,
    error_modal: ErrorModal,
    status_colors: StatusColors,
    active_tab: Tab,
    gui_state: SharedGuiState,
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler.update("Quit Cobra Commander?", ctx);

        let prev_tab = self.active_tab;

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Midi, "MIDI");
                ui.selectable_value(&mut self.active_tab, Tab::Osc, "OSC");
                ui.selectable_value(&mut self.active_tab, Tab::Clocks, "Clocks");
                ui.selectable_value(&mut self.active_tab, Tab::Animation, "Animation");
            });
        });

        // Notify the show when the visualizer tab becomes active or inactive.
        if self.active_tab != prev_tab {
            self.gui_state
                .visualizer_active
                .store(self.active_tab == Tab::Animation, Ordering::Relaxed);
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Clocks => {
                let clock_status = self.gui_state.clock_status.load();
                ClockPanel {
                    ctx: GuiContext {
                        error_modal: &mut self.error_modal,
                        client: &self.client,
                    },
                    state: &mut self.clock_panel,
                    clock_status: &clock_status,
                    status_colors: &self.status_colors,
                }
                .ui(ui);
            }
            Tab::Midi => {
                let midi_slots = self.gui_state.midi_slots.load();
                MidiPanel {
                    ctx: GuiContext {
                        error_modal: &mut self.error_modal,
                        client: &self.client,
                    },
                    state: &mut self.midi_panel,
                    slots: &midi_slots,
                    status_colors: &self.status_colors,
                }
                .ui(ui);
            }
            Tab::Osc => {
                let clients = self.gui_state.osc_clients.load();
                let mut ctx = GuiContext {
                    error_modal: &mut self.error_modal,
                    client: &self.client,
                };
                osc_panel::ui(ui, &mut ctx, &self.gui_state.osc_listen_addr, &clients);
            }
            Tab::Animation => {
                let state = self.gui_state.animation_state.load();
                self.visualizer_panel.ui(ui, &state);
            }
        });

        self.error_modal.ui(ctx);
    }
}

pub fn run_config_gui(
    client: CommandClient,
    zmq_ctx: zmq::Context,
    gui_state: SharedGuiState,
) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 300.0]),
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
                visualizer_panel: VisualizerPanelState::default(),
                client,
                close_handler: CloseHandler::default(),
                error_modal: ErrorModal::default(),
                status_colors: StatusColors::default(),
                active_tab: Tab::default(),
                gui_state,
            }))
        }),
    )
    .unwrap();
    Ok(())
}
