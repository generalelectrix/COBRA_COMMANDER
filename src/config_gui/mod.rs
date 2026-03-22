mod clock_panel;
mod midi_panel;

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::gui_state::SharedGuiState;
use crate::ui_util::CloseHandler;
use clock_panel::ClockPanel;

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Config,
    Midi,
}

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanel,
    close_handler: CloseHandler,
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

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::Config => self.clock_panel.ui(ui, &self.client),
            Tab::Midi => {
                let midi_slots = self.gui_state.midi_slots.load();
                midi_panel::ui(ui, &midi_slots);
            }
        });
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
    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ConfigApp {
                clock_panel: ClockPanel::new(zmq_ctx),
                client,
                close_handler: CloseHandler::default(),
                active_tab: Tab::default(),
                gui_state,
            }))
        }),
    )
    .unwrap();
    Ok(())
}
