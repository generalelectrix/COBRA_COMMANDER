use anyhow::Result;
use log::error;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use number::Phase;
use serde::{Deserialize, Serialize};
use tunnels::{animation::Animation, clock_server::SharedClockData};
use zero_configure::pub_sub::{PublisherService, SubscriberService};

pub fn run_animation_visualizer() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([350.0, 200.0]),
        ..Default::default()
    };
    eframe::run_native(
        "My egui App with a plot",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let state = start_service(zmq::Context::new(), move || ctx.request_repaint())?;

            Ok(Box::new(AnimationVisualizer { state }))
        }),
    )
    .unwrap();
    Ok(())
}

#[derive(Default)]
struct AnimationVisualizer {
    state: Arc<Mutex<AnimationServiceState>>,
}

impl eframe::App for AnimationVisualizer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let borrowed = self.state.lock().unwrap();

        let get_val = |phase: f64| {
            borrowed.animation.get_value(
                Phase::new(phase),
                0,
                &borrowed.clocks.clock_bank,
                borrowed.clocks.audio_envelope,
            )
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            Plot::new("My Plot")
                .default_x_bounds(0.0, 1.0)
                .default_y_bounds(-1.0, 1.0)
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(
                        "curve",
                        PlotPoints::from_explicit_callback(get_val, 0.0..0.999, 1000),
                    ));
                });
        });
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct AnimationServiceState {
    pub animation: Animation,
    pub clocks: SharedClockData,
}

fn start_service(
    ctx: zmq::Context,
    mut on_update: impl FnMut() + Send + 'static,
) -> Result<Arc<Mutex<AnimationServiceState>>> {
    println!("Browsing for animation providers...");
    let service = animation_subscriber(ctx);
    let provider = loop {
        thread::sleep(Duration::from_secs(2));
        let providers = service.list();
        if let Some(provider) = providers.into_iter().next() {
            break provider;
        } else {
            println!("No animation providers found; trying again.");
            continue;
        }
    };
    println!("Connecting to {provider}...");
    let mut receiver = service.subscribe(&provider, None)?;
    let storage = Arc::new(Mutex::new(AnimationServiceState::default()));
    let storage_handle = storage.clone();
    thread::spawn(move || loop {
        let msg = match receiver.receive_msg(true) {
            Err(e) => {
                error!("animation receive error: {e}");
                continue;
            }
            Ok(None) => {
                continue;
            }
            Ok(Some(msg)) => msg,
        };
        *storage_handle.lock().unwrap() = msg;
        on_update();
    });
    println!("Connected to {provider}.");
    Ok(storage)
}

pub type AnimationPublisher = PublisherService<AnimationServiceState>;
pub type AnimationSubscriber = SubscriberService<AnimationServiceState>;

const SERVICE_NAME: &str = "current_animator_state";
const PORT: u16 = 9091;

pub fn animation_publisher(ctx: &zmq::Context) -> Result<AnimationPublisher> {
    PublisherService::new(ctx, SERVICE_NAME, PORT)
}

pub fn animation_subscriber(ctx: zmq::Context) -> AnimationSubscriber {
    SubscriberService::new(ctx, SERVICE_NAME.to_string())
}
