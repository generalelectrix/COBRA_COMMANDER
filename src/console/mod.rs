//! egui-based console GUI for live show management.

mod app;
pub mod command;
pub mod state;

use std::sync::{Arc, Mutex, mpsc};

use anyhow::Result;
use eframe::egui;

use app::ConsoleApp;
use command::ConsoleCommand;
use state::{ConsoleState, FixtureTypeMeta, GroupSummary, PatchSummary};

use crate::control::Controller;
use crate::fixture::Patch;
use crate::fixture::patch::PATCHERS;

/// Handle held by the Show to communicate with the console.
pub struct ConsoleHandle {
    /// Shared state: Show writes snapshots, GUI reads them.
    pub state: Arc<Mutex<ConsoleState>>,
    /// Receive commands from the GUI.
    pub commands: mpsc::Receiver<ConsoleCommand>,
}

/// Handle held by the console GUI app.
pub struct ConsoleAppHandle {
    /// Shared state: read snapshots written by Show.
    pub state: Arc<Mutex<ConsoleState>>,
    /// Send commands to the Show.
    pub commands: mpsc::Sender<ConsoleCommand>,
}

/// Create a paired set of handles for Show <-> Console communication.
pub fn create_console_handles(initial_state: ConsoleState) -> (ConsoleHandle, ConsoleAppHandle) {
    let state = Arc::new(Mutex::new(initial_state));
    let (tx, rx) = mpsc::channel();
    (
        ConsoleHandle {
            state: Arc::clone(&state),
            commands: rx,
        },
        ConsoleAppHandle {
            state,
            commands: tx,
        },
    )
}

/// Build a ConsoleState snapshot from the current Patch and Controller.
pub fn console_state_snapshot(patch: &Patch, _controller: &Controller) -> ConsoleState {
    let fixture_types: Vec<FixtureTypeMeta> = PATCHERS
        .iter()
        .map(|p| FixtureTypeMeta {
            name: p.name.to_string(),
            group_options: (p.group_options)(),
            patch_options: (p.patch_options)(),
        })
        .collect();

    let groups: Vec<GroupSummary> = patch
        .iter_with_keys()
        .map(|(key, group)| {
            let patches: Vec<PatchSummary> = group
                .fixture_configs()
                .iter()
                .map(|cfg| PatchSummary {
                    addr: cfg.dmx_index.map(|i| (i + 1) as u32),
                    universe: cfg.universe,
                    mirror: cfg.mirror,
                    channel_count: cfg.channel_count,
                    options: vec![],
                })
                .collect();

            GroupSummary {
                key: key.to_string(),
                fixture_type: group.qualified_name().to_string(),
                channel: patch.channels().any(|ch| ch == key),
                color_organ: false,
                options: vec![],
                patches,
            }
        })
        .collect();

    ConsoleState {
        fixture_types,
        groups,
        osc_clients: vec![],
        midi_inputs: vec![],
        last_error: None,
    }
}

/// Run the console GUI on the current thread.
pub fn run_console(handle: ConsoleAppHandle) -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    let state = handle.state;
    let commands = handle.commands;

    eframe::run_native(
        "Cobra Commander Console",
        options,
        Box::new(move |_cc| Ok(Box::new(ConsoleApp::new(state, commands)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
