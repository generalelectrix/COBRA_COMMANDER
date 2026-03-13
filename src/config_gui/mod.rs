mod clock_panel;

use anyhow::Result;
use eframe::egui;

use crate::control::CommandClient;
use crate::ui_util::CloseHandler;
use clock_panel::ClockPanel;

struct ConfigApp {
    client: CommandClient,
    clock_panel: ClockPanel,
    close_handler: CloseHandler,
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler
            .update("Close configuration?", ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Show Configuration");
            ui.separator();
            self.clock_panel.ui(ui, &self.client);
        });
    }
}

pub fn run_config_gui(client: CommandClient, zmq_ctx: zmq::Context) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Cobra Commander Configuration",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ConfigApp {
                clock_panel: ClockPanel::new(zmq_ctx),
                client,
                close_handler: CloseHandler::default(),
            }))
        }),
    )
    .unwrap();
    Ok(())
}
