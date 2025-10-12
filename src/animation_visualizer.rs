use anyhow::Result;
use log::error;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Points};
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
        "Cobra Commander Animation Visualizer",
        options,
        Box::new(|cc| {
            let ctx = cc.egui_ctx.clone();
            let state = start_service(zmq::Context::new(), move || ctx.request_repaint())?;

            Ok(Box::new(AnimationVisualizer {
                state,
                preview: vec![],
                live: vec![],
                dots: vec![],
                close_handler: Default::default(),
            }))
        }),
    )
    .unwrap();
    Ok(())
}

struct AnimationVisualizer {
    /// Current animation service state.
    state: Arc<Mutex<AnimationServiceState>>,
    /// Buffer for rendering the "preview" visualization of the waveform; this
    /// follows the phase evolution but always renders with an amplitude of 1.
    preview: Vec<PlotPoint>,
    /// Buffer for rendering the "live" visualization of the waveform.
    live: Vec<PlotPoint>,
    /// Buffer for rendering the individual dots for each fixture.
    dots: Vec<PlotPoint>,

    /// Handle closing the window.
    close_handler: CloseHandler,
}

impl eframe::App for AnimationVisualizer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler
            .update("Are you sure you want to quit the visualizer?", ctx);

        let state = self.state.lock().unwrap();

        // FIXME: deduplicate this logic between here and FixtureGroup.
        let phase_offset_per_fixture = 1.0 / state.fixture_count as f64;

        const NUM_WAVE_POINTS: usize = 1000;

        // Smooth wave values; we derive an offset index to correctly
        // render the offsetting behavior for noise waveforms.
        self.preview.clear();
        self.preview.extend((0..NUM_WAVE_POINTS).map(|i| {
            let phase = i as f64 / NUM_WAVE_POINTS as f64;
            let offset_index = (phase / phase_offset_per_fixture) as usize;
            let y = state.animation.get_unit_value(
                Phase::new(phase),
                offset_index,
                &state.clocks.clock_bank,
            );
            PlotPoint::new(phase, y)
        }));

        // The "live" waveform is just the rescaled version.
        self.live.clear();
        self.live.extend(self.preview.iter().map(|point| {
            PlotPoint::new(
                point.x,
                state.animation.scale_value(
                    &state.clocks.clock_bank,
                    state.clocks.audio_envelope,
                    point.y,
                ),
            )
        }));

        self.dots.clear();
        self.dots.extend((0..state.fixture_count).map(|i| {
            let phase = i as f64 * phase_offset_per_fixture;
            let y = state.animation.get_value(
                Phase::new(phase),
                i,
                &state.clocks.clock_bank,
                state.clocks.audio_envelope,
            );
            PlotPoint::new(phase, y)
        }));

        egui::CentralPanel::default().show(ctx, |ui| {
            Plot::new("Animation")
                .default_x_bounds(0.0, 1.0)
                .default_y_bounds(-1.0, 1.0)
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        Line::new("Unit Waveform", PlotPoints::Borrowed(&self.preview))
                            .color(Color32::DARK_RED)
                            .width(2.0),
                    );
                    plot_ui.line(
                        Line::new("Scaled Waveform", PlotPoints::Borrowed(&self.live))
                            .color(Color32::WHITE)
                            .width(2.0),
                    );
                    plot_ui.points(
                        Points::new("Fixture Values", PlotPoints::Borrowed(&self.dots))
                            .color(Color32::CYAN)
                            .radius(5.0),
                    );
                });
        });
    }
}

#[derive(Default)]
struct CloseHandler {
    show_confirmation_dialog: bool,
    allowed_to_close: bool,
}

impl CloseHandler {
    fn update(&mut self, quit_prompt: &str, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allowed_to_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_confirmation_dialog = true;
        }

        if self.show_confirmation_dialog {
            egui::Window::new(quit_prompt)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("No").clicked() {
                            self.show_confirmation_dialog = false;
                            self.allowed_to_close = false;
                        }

                        if ui.button("Yes").clicked() {
                            self.show_confirmation_dialog = false;
                            self.allowed_to_close = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct AnimationServiceState {
    /// Animation state.
    pub animation: Animation,
    /// Clock state.
    pub clocks: SharedClockData,
    /// The number of fixtures patched in the current group.
    pub fixture_count: usize,
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
    let storage = Arc::new(Mutex::new(AnimationServiceState {
        fixture_count: 1,
        ..Default::default()
    }));
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
