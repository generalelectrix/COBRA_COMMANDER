mod clock_panel;
mod midi_panel;

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::gui_state::SharedGuiState;
use crate::ui_util::{CloseHandler, ErrorModal, GuiContext};
use clock_panel::{ClockPanel, ClockPanelState};

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Config,
    Midi,
}

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanelState,
    close_handler: CloseHandler,
    error_modal: ErrorModal,
    active_tab: Tab,
    gui_state: SharedGuiState,
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler.update("Quit Cobra Commander?", ctx);

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Config, "Config");
                ui.selectable_value(&mut self.active_tab, Tab::Midi, "MIDI");
            });
        });

        let clock_status = self.gui_state.clock_status.load();

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Config => {
                ClockPanel {
                    ctx: GuiContext {
                        error_modal: &mut self.error_modal,
                        client: &self.client,
                    },
                    state: &mut self.clock_panel,
                    clock_status: &clock_status,
                }
                .ui(ui);
            }
            Tab::Midi => {
                let midi_slots = self.gui_state.midi_slots.load();
                midi_panel::ui(ui, &midi_slots);
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
                client,
                close_handler: CloseHandler::default(),
                error_modal: ErrorModal::default(),
                active_tab: Tab::default(),
                gui_state,
            }))
        }),
    )
    .unwrap();
    Ok(())
}
