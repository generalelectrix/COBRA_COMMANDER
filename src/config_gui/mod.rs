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
mod audio_panel;
mod clock_panel;
mod dmx_debug_panel;
mod dmx_panel;
mod midi_panel;
mod osc_panel;
mod patch_panel;
mod welcome;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;
use log::error;
use midi_harness::install_midi_device_change_handler;
use tunnels::audio::EnvelopeStreams;
use tunnels_lib::repaint::RepaintSignal;

use crate::LOG_SCROLLBACK_PER_SEVERITY;
use crate::clocks::Clocks;
use crate::control::{CommandClient, Controller};
use crate::fixture::Patch;
use crate::gui_state::{ClockStatus, DMX_DEBUG_NOT_WATCHING, GuiState, SharedGuiState};
use crate::midi::ControlHandler;
use crate::preview::Previewer;
use crate::show::Show;
use crate::ui_util::GuiContext;
use animation_panel::VisualizerPanelState;
use audio_panel::AudioPanelState;
use clock_panel::{ClockPanel, ClockPanelState};
use dmx_panel::{DmxPortPanel, DmxPortPanelState};
use gui_common::envelope_viewer::EnvelopeViewerState;
use gui_common::log_status::{self, LogRecord, LogStatusPanel, LogStatusState};
use gui_common::{CloseHandler, MessageModal};
use midi_panel::{MidiPanel, MidiPanelState};
use patch_panel::{PatchPanel, PatchPanelState};
use welcome::WelcomeResult;

fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::BLACK;
    visuals.window_fill = egui::Color32::BLACK;
    visuals.extreme_bg_color = egui::Color32::from_rgb(20, 20, 20);
    visuals.faint_bg_color = egui::Color32::from_rgb(10, 10, 10);
    ctx.set_visuals(visuals);
}

/// Hash source for the DMX output debug window's [`egui::ViewportId`]. Shared
/// between the `show_viewport_deferred` call and the repaint signal that wakes
/// that viewport, so both refer to the same window.
const DMX_DEBUG_VIEWPORT: &str = "dmx_output_debug";

#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    Patch,
    Dmx,
    Midi,
    Osc,
    ClocksAudio,
    Animation,
    Status,
}

struct ConsoleApp {
    client: CommandClient,
    show_file_path: PathBuf,
    source_panel: ClockPanelState,
    audio_panel: AudioPanelState,
    envelope_viewer: EnvelopeViewerState,
    envelope_streams_rx: Receiver<EnvelopeStreams>,
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
    /// Whether the DMX output debug window is open. Arc<AtomicBool> so the
    /// deferred viewport closure ('static + Send + Sync) can signal "close".
    dmx_debug_open: Arc<AtomicBool>,
    /// Universe selected in the DMX output debug window. Arc<AtomicUsize> so the
    /// deferred viewport closure can write the combo box selection; the main
    /// loop reads it to drive the Show's watch signal.
    dmx_debug_selected: Arc<AtomicUsize>,
    patchers: Vec<crate::fixture::patch::Patcher>,
    close_handler: CloseHandler,
    modal: MessageModal,
    active_tab: Tab,
    gui_state: SharedGuiState,
    log_status: LogStatusState,
}

impl eframe::App for ConsoleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.close_handler.update("Quit Cobra Commander?", ctx);

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Patch, "Patch");
                ui.selectable_value(&mut self.active_tab, Tab::Dmx, "DMX");
                ui.selectable_value(&mut self.active_tab, Tab::Midi, "MIDI");
                ui.selectable_value(&mut self.active_tab, Tab::Osc, "OSC");
                ui.selectable_value(&mut self.active_tab, Tab::ClocksAudio, "Clocks/Audio");
                ui.selectable_value(&mut self.active_tab, Tab::Animation, "Animation");
                if log_status::status_tab(ui, self.active_tab == Tab::Status, &self.log_status) {
                    self.active_tab = Tab::Status;
                }
            });
        });

        // Let the drain thread know whether the live log view is in front, so
        // it wakes the GUI on plain records only while the Status tab is open.
        self.log_status.set_viewing(self.active_tab == Tab::Status);

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

        // Tell the show which universe to snapshot for the DMX output debug
        // window — only while it is open. This is the only trigger for the
        // show's snapshot work, so closing the window stops it entirely.
        let dmx_debug_open = self.dmx_debug_open.load(Ordering::Relaxed);
        self.gui_state.dmx_debug_watch.store(
            if dmx_debug_open {
                self.dmx_debug_selected.load(Ordering::Relaxed)
            } else {
                DMX_DEBUG_NOT_WATCHING
            },
            Ordering::Relaxed,
        );

        // DMX output debug — separate OS window via deferred viewport.
        if dmx_debug_open {
            let gui_state = self.gui_state.clone();
            let selected = self.dmx_debug_selected.clone();
            let open_flag = self.dmx_debug_open.clone();
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(DMX_DEBUG_VIEWPORT),
                egui::ViewportBuilder::default()
                    .with_title("DMX Output Monitor")
                    // Roughly fits the 16x32 grid + selector at default style.
                    .with_inner_size(egui::vec2(760.0, 705.0)),
                move |ctx, _class| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        dmx_debug_panel::dmx_debug_panel_ui(ui, &gui_state, &selected);
                    });
                    if ctx.input(|i| i.viewport().close_requested()) {
                        open_flag.store(false, Ordering::Relaxed);
                    }
                },
            );
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            Tab::ClocksAudio => {
                let clock_status = self.gui_state.clock_status.load();
                ClockPanel {
                    ctx: GuiContext {
                        modal: &mut self.modal,
                        client: &self.client,
                    },
                    state: &mut self.source_panel,
                    clock_status: &clock_status,
                }
                .ui(ui);

                match &**clock_status {
                    ClockStatus::Internal { .. } => {
                        ui.add_space(8.0);
                        ui.separator();
                        let audio_state = self.gui_state.audio_state.load();
                        audio_panel::render_audio_panel(
                            ui,
                            GuiContext {
                                modal: &mut self.modal,
                                client: &self.client,
                            },
                            &mut self.audio_panel,
                            &audio_state,
                        );
                        if audio_state.device_name != tunnels::audio::OFFLINE_DEVICE_NAME {
                            while let Ok(streams) = self.envelope_streams_rx.try_recv() {
                                self.envelope_viewer.set_envelope_streams(streams);
                            }
                            ui.add_space(8.0);
                            ui.separator();
                            self.envelope_viewer.ui(ui);
                        } else {
                            self.envelope_viewer.set_open(false);
                        }
                    }
                    ClockStatus::Remote { .. } => {
                        self.envelope_viewer.set_open(false);
                    }
                }
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
                    local_ip: **self.gui_state.osc_local_ip.load(),
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
                    debug_open: &self.dmx_debug_open,
                }
                .ui(ui);
            }
            Tab::Status => {
                LogStatusPanel {
                    state: &mut self.log_status,
                }
                .ui(ui);
            }
        });

        self.modal.ui(ctx);
    }
}

/// Single entry point for the console. Runs the welcome screen, initializes
/// the show, and runs the main console GUI.
pub fn run_console(log_rx: Receiver<LogRecord>) -> Result<()> {
    // Phase 1: Welcome screen. The OSC receive port is bound here so a port
    // conflict is recoverable via a prompt instead of crashing show init.
    let welcome_result = welcome::run_welcome(crate::osc::DEFAULT_RECEIVE_PORT)?;

    let (show_file_path, initial_configs, osc_socket, bound_port) = match welcome_result {
        WelcomeResult::LoadShow {
            path,
            configs,
            bound,
        } => (path, configs, bound.socket, bound.port),
        WelcomeResult::NewShow { path, bound } => (path, vec![], bound.socket, bound.port),
        WelcomeResult::Quit => return Ok(()),
    };

    // Phase 2: Create infrastructure that doesn't depend on egui_ctx.
    let osc_local_ip = crate::local_ip_watch::current_ip();

    let (send_control_msg, recv_control_msg) = channel();
    let command_client = CommandClient::new(send_control_msg.clone());

    // NOTE: this MUST be called before any other MIDI functions.
    install_midi_device_change_handler(ControlHandler(send_control_msg.clone()))?;

    let controller = Controller::new(
        osc_socket,
        vec![],
        vec![],
        send_control_msg,
        recv_control_msg,
    )?;

    // Move-once values for the eframe creator closure.
    let mut startup = Some((
        controller,
        osc_local_ip,
        bound_port,
        initial_configs,
        log_rx,
    ));

    // Phase 3: Run the console GUI. GuiState construction (which needs a
    // RepaintSignal built from cc.egui_ctx) and the show-thread spawn live
    // inside the creator closure.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 650.0])
            .with_icon(std::sync::Arc::new(egui::IconData::default())),
        ..Default::default()
    };
    eframe::run_native(
        "Cobra Commander",
        options,
        Box::new(move |cc| {
            stage_theme::apply(&cc.egui_ctx);

            let (controller, osc_local_ip, bound_port, initial_configs, log_rx) =
                startup.take().expect("creator closure called once");

            let repaint: RepaintSignal = {
                let ctx = cc.egui_ctx.clone();
                Arc::new(move || ctx.request_repaint())
            };

            // Build the in-GUI log surfaces and spawn the drain thread that
            // moves captured records into scrollback and fires the repaint.
            let log_alert = Arc::new(log_status::LogAlert::new(repaint.clone()));
            let scrollback = Arc::new(Mutex::new(log_status::Scrollback::new(
                LOG_SCROLLBACK_PER_SEVERITY,
            )));
            let viewing = Arc::new(AtomicBool::new(false));
            log_status::spawn_drain_thread(
                log_rx,
                scrollback.clone(),
                log_alert.clone(),
                viewing.clone(),
            );
            let log_status = LogStatusState::new(log_alert, scrollback, viewing);

            // The DMX debug window is a separate deferred viewport, so a plain
            // root `request_repaint()` won't re-render it. Its snapshot Notified
            // gets a signal that also wakes the debug viewport (so new ~4fps
            // snapshots show up immediately) and the root (to keep the watch
            // signal in sync after a universe change).
            let dmx_debug_repaint: RepaintSignal = {
                let ctx = cc.egui_ctx.clone();
                Arc::new(move || {
                    ctx.request_repaint();
                    ctx.request_repaint_of(egui::ViewportId::from_hash_of(DMX_DEBUG_VIEWPORT));
                })
            };

            let gui_state: SharedGuiState = Arc::new(GuiState::new(
                vec![],
                ClockStatus::Internal {
                    audio_device: tunnels::audio::OFFLINE_DEVICE_NAME.into(),
                },
                osc_local_ip,
                repaint,
                dmx_debug_repaint,
            ));

            // Keep the displayed listen address current as the host's local IP
            // changes underneath us (interface swap, VPN, DHCP renew).
            crate::local_ip_watch::spawn(gui_state.clone());

            let (envelope_tx, envelope_rx) = channel::<EnvelopeStreams>();

            let show_gui_state = gui_state.clone();
            let show_envelope_tx = envelope_tx.clone();
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
                let clocks = match Clocks::internal(None, show_envelope_tx.clone()) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Clocks initialization error: {e:#}");
                        return;
                    }
                };
                let show = Show::new(
                    patch,
                    initial_configs,
                    controller,
                    dmx,
                    clocks,
                    Previewer::default(),
                    show_gui_state,
                    show_envelope_tx,
                );
                match show {
                    Ok(mut show) => show.run(),
                    Err(e) => error!("Show initialization error: {e:#}"),
                }
            });

            let initial_clock_status = ClockStatus::Internal {
                audio_device: tunnels::audio::OFFLINE_DEVICE_NAME.into(),
            };
            let devices = tunnels::audio::AudioInput::devices().unwrap_or_default();
            let audio_panel = AudioPanelState::new(devices);

            Ok(Box::new(ConsoleApp {
                source_panel: ClockPanelState::new(&initial_clock_status),
                audio_panel,
                envelope_viewer: EnvelopeViewerState::new(),
                envelope_streams_rx: envelope_rx,
                midi_panel: MidiPanelState::new(),
                visualizer_panel: Arc::new(Mutex::new(VisualizerPanelState::default())),
                visualizer_detached: Arc::new(AtomicBool::new(false)),
                osc_panel: osc_panel::OscPanelState::new(bound_port),
                patch_panel: PatchPanelState::new(),
                dmx_panel: DmxPortPanelState::new(),
                dmx_debug_open: Arc::new(AtomicBool::new(false)),
                dmx_debug_selected: Arc::new(AtomicUsize::new(0)),
                patchers: Patch::menu(),
                client: command_client,
                show_file_path,
                close_handler: CloseHandler::default(),
                modal: MessageModal::default(),
                active_tab: Tab::default(),
                gui_state,
                log_status,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe console window failed: {e}"))?;
    Ok(())
}
