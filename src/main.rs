use anyhow::{Result, bail};
use clap::Parser;
use clocks::Clocks;
use fixture::Patch;
use local_ip_address::local_ip;
use log::{LevelFilter, error};
use midi::Device;
use midi_harness::install_midi_device_change_handler;
use rust_dmx::{DmxPort, OfflineDmxPort, available_ports};
use simplelog::{Config as LogConfig, SimpleLogger};
use std::sync::mpsc::channel;
use std::time::Duration;
use tunnels::midi::list_ports;
use zmq::Context;

use crate::animation_visualizer::run_animation_visualizer;
use crate::cli::*;
use crate::control::{CommandClient, Controller};
use crate::midi::ControlHandler;
use crate::preview::Previewer;
use crate::show::Show;

mod animation;
mod animation_visualizer;
mod channel;
mod cli;
mod clock_service;
mod clocks;
mod color;
mod config;
mod config_gui;
mod control;
mod dmx;
mod fixture;
mod master;
mod midi;
mod osc;
mod preview;
mod show;
mod strobe;
mod ui_util;
mod util;
mod wled;

fn main() -> Result<()> {
    let args = Cli::try_parse()?;

    let log_level = if args.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };

    SimpleLogger::init(log_level, LogConfig::default())?;

    match args.command {
        Command::Run(args) => run_show(args),
        Command::Check(args) => check_patch(args),
        Command::Viz => run_animation_visualizer(),
        Command::Fix(args) => fixture_help(args),
    }
}

const ARTNET_POLL_TIMEOUT: Duration = Duration::from_secs(10);

fn run_show(args: RunArgs) -> Result<()> {
    let zmq_ctx = Context::new();

    match local_ip() {
        Ok(ip) => println!("Listening for OSC at {}:{}.", ip, args.osc_receive_port),
        Err(e) => println!("Unable to fetch local IP address: {e}."),
    }

    let (send_control_msg, recv_control_msg) = channel();
    let command_client = CommandClient::new(send_control_msg.clone(), zmq_ctx.clone());

    // NOTE: this MUST be called before any other MIDI functions.
    install_midi_device_change_handler(ControlHandler(send_control_msg.clone()))?;

    if args.gui && !args.quickstart {
        // GUI mode: egui on main thread, Show on worker thread.
        let gui_client = command_client.clone();
        let gui_zmq = zmq_ctx.clone();

        std::thread::spawn(move || {
            if let Err(e) = run_show_worker(args, send_control_msg, recv_control_msg, zmq_ctx) {
                error!("Show worker error: {e:#}");
            }
        });

        config_gui::run_config_gui(gui_client, gui_zmq)?;
    } else {
        // Non-GUI path: existing behavior unchanged.
        run_show_inline(args, command_client, send_control_msg, recv_control_msg, zmq_ctx)?;
    }

    Ok(())
}

/// Build and run the show on the current thread (non-GUI path).
fn run_show_inline(
    args: RunArgs,
    command_client: CommandClient,
    send_control_msg: std::sync::mpsc::Sender<crate::control::ControlMessage>,
    recv_control_msg: std::sync::mpsc::Receiver<crate::control::ControlMessage>,
    zmq_ctx: Context,
) -> Result<()> {
    let patch = Patch::from_file(&args.patch_file)?;
    let clocks = Clocks::internal(None);

    let midi_devices = if args.quickstart {
        let (midi_inputs, midi_outputs) = list_ports()?;
        let devices = Device::auto_configure(true, &midi_inputs, &midi_outputs);
        if devices.is_empty() {
            println!("No known MIDI devices were automatically discovered.");
        } else {
            println!("These known MIDI devices were found:");
            for d in &devices {
                println!("  - {}", d.device);
            }
        }
        devices
    } else {
        vec![]
    };

    let controller = Controller::new(
        args.osc_receive_port,
        vec![],
        midi_devices,
        send_control_msg,
        recv_control_msg,
    )?;

    let universe_count = patch.universe_count();
    println!("This show requires {universe_count} universe(s).");

    let dmx_ports: Vec<Box<dyn DmxPort>> = if args.quickstart {
        if args.artnet {
            println!("Searching for artnet ports...");
        }
        let available = available_ports(args.artnet.then_some(ARTNET_POLL_TIMEOUT))?;
        let mut ports = Vec::new();
        for (i, port) in (0..universe_count).zip(available.into_iter().rev().chain(
            std::iter::repeat_with(|| Box::new(OfflineDmxPort) as Box<dyn DmxPort>),
        )) {
            println!("Assigning universe {i} to port {port}.");
            ports.push(port);
        }
        ports
    } else {
        (0..universe_count)
            .map(|_| Box::new(OfflineDmxPort) as Box<dyn DmxPort>)
            .collect()
    };

    let mut show = Show::new(
        patch,
        args.patch_file,
        controller,
        dmx_ports,
        clocks,
        None,
        args.cli_preview
            .then(Previewer::terminal)
            .unwrap_or_default(),
        args.master_strobe_channel,
        zmq_ctx,
    )?;

    if !args.quickstart {
        let cli_client = command_client.clone();
        std::thread::spawn(move || {
            if let Err(e) = cli::run_cli_configuration(cli_client, universe_count) {
                error!("CLI configuration error: {e:#}");
            }
        });
    }

    println!("Running show.");
    show.run();

    Ok(())
}

/// Build and run the show on a worker thread (GUI path).
fn run_show_worker(
    args: RunArgs,
    send_control_msg: std::sync::mpsc::Sender<crate::control::ControlMessage>,
    recv_control_msg: std::sync::mpsc::Receiver<crate::control::ControlMessage>,
    zmq_ctx: Context,
) -> Result<()> {
    let patch = Patch::from_file(&args.patch_file)?;
    let clocks = Clocks::internal(None);

    let controller = Controller::new(
        args.osc_receive_port,
        vec![],
        vec![],
        send_control_msg,
        recv_control_msg,
    )?;

    let universe_count = patch.universe_count();
    println!("This show requires {universe_count} universe(s).");

    let dmx_ports: Vec<Box<dyn DmxPort>> = (0..universe_count)
        .map(|_| Box::new(OfflineDmxPort) as Box<dyn DmxPort>)
        .collect();

    let mut show = Show::new(
        patch,
        args.patch_file,
        controller,
        dmx_ports,
        clocks,
        None,
        Previewer::default(),
        args.master_strobe_channel,
        zmq_ctx,
    )?;

    println!("Running show.");
    show.run();

    Ok(())
}

fn check_patch(args: CheckArgs) -> Result<()> {
    let patch = Patch::from_file(&args.patch_file)?;
    println!("Patch is OK.");
    if args.describe_controls {
        println!();
        for (key, group) in patch.iter_with_keys() {
            let controls = group.describe_controls();
            println!(
                "{key} ({} control{}):",
                controls.len(),
                if controls.len() == 1 { "" } else { "s" }
            );
            for control in &controls {
                println!("  {}: {}", control.name, control.control_type);
            }
            println!();
        }
    }
    Ok(())
}

fn fixture_help(args: FixArgs) -> Result<()> {
    let fixtures = Patch::menu();
    if args.all {
        for f in fixtures {
            println!("{f}");
        }
        return Ok(());
    }
    let Some(fixture_name) = args.fixture else {
        bail!("specify a single fixture to get info about or pass --all");
    };
    let Some(fixture) = fixtures.into_iter().find(|f| f.name.0 == fixture_name) else {
        bail!("unknown fixture '{}'", fixture_name);
    };
    println!("{fixture}");
    Ok(())
}
