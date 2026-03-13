mod clock_panel;

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::ui_util::CloseHandler;
use clock_panel::ClockPanel;

#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Config,
}

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanel,
    close_handler: CloseHandler,
    active_tab: Tab,
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler
            .update("Quit Cobra Commander?", ctx);

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Config, "Config");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                Tab::Config => self.clock_panel.ui(ui, &self.client),
            }
        });
    }
}

pub fn run_config_gui(client: CommandClient, zmq_ctx: zmq::Context) -> Result<()> {
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
            }))
        }),
    )
    .unwrap();
    Ok(())
}
