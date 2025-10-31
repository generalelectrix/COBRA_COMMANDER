use anyhow::Context as _;
use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use clock_service::prompt_start_clock_service;
use clocks::Clocks;
use fixture::Patch;
use local_ip_address::local_ip;
use log::LevelFilter;
use midi::Device;
use osc::prompt_osc_config;
use rust_dmx::select_port;
use simplelog::{Config as LogConfig, SimpleLogger};
use std::env::current_exe;
use std::fs::File;
use std::path::{Path, PathBuf};
use strum_macros::Display;
use tunnels::audio::prompt_audio;
use tunnels::audio::AudioInput;
use tunnels::midi::prompt_midi;
use tunnels::midi::{list_ports, DeviceSpec};
use tunnels_lib::prompt::{prompt_bool, prompt_indexed_value};
use zmq::Context;

use crate::animation_visualizer::{
    animation_publisher, run_animation_visualizer, AnimationPublisher,
};
use crate::config::FixtureGroupConfig;
use crate::control::Controller;
use crate::midi::ColorOrgan;
use crate::preview::Previewer;
use crate::show::Show;

mod animation;
mod animation_visualizer;
mod channel;
mod clock_service;
mod clocks;
mod color;
mod config;
mod control;
mod dmx;
mod fixture;
mod master;
mod midi;
mod osc;
mod preview;
mod show;
mod strobe;
mod util;
mod wled;

#[derive(Parser)]
#[command(about)]
struct Cli {
    /// If true, provide verbose logging.
    #[arg(long)]
    debug: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Display)]
#[strum(serialize_all = "lowercase")]
enum Command {
    /// Run the controller.
    Run(RunArgs),

    /// Check that the provided patch file is valid, then quit.
    Check(CheckArgs),

    /// Run the animation visualizer.
    Viz,

    /// Get fixture info.
    Fix(FixArgs),
}

#[derive(Args)]
struct RunArgs {
    /// Path to a YAML file containing the fixture patch.
    patch_file: PathBuf,

    /// Check that the provided patch file is valid and quit.
    #[arg(long)]
    check_patch: bool,

    /// The port on which to listen for OSC messages.
    #[arg(long, default_value_t = 8000)]
    osc_receive_port: u16,

    /// If true, render fixture preview into the CLI.
    #[arg(long)]
    cli_preview: bool,
}

#[derive(Args)]
struct CheckArgs {
    /// Path to a YAML file containing the fixture patch.
    patch_file: PathBuf,
}

#[derive(Args)]
struct FixArgs {
    /// Show info for all registered fixture types.
    #[arg(long)]
    all: bool,

    /// Show info for one fixture type.
    fixture: Option<String>,
}

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

fn run_show(args: RunArgs) -> Result<()> {
    let patch = Patch::from_file(&args.patch_file)?;

    let zmq_ctx = Context::new();

    let clocks = if let Some(clock_service) = prompt_start_clock_service(zmq_ctx.clone())? {
        Clocks::Service(clock_service)
    } else {
        let audio_device = prompt_audio()?
            .map(|device_name| AudioInput::new(Some(device_name)))
            .transpose()?;

        Clocks::internal(audio_device)
    };

    let internal_clocks = matches!(clocks, Clocks::Internal { .. });

    let animation_service = prompt_start_animation_service(&zmq_ctx)?;

    match local_ip() {
        Ok(ip) => println!("Listening for OSC at {}:{}.", ip, args.osc_receive_port),
        Err(e) => println!("Unable to fetch local IP address: {e}."),
    }

    let osc_controllers = prompt_osc_config(args.osc_receive_port)?.unwrap_or_default();

    let (midi_inputs, midi_outputs) = list_ports()?;
    let mut midi_devices = Device::auto_configure(internal_clocks, &midi_inputs, &midi_outputs);
    if midi_devices.is_empty() {
        println!("No known MIDI devices were automatically discovered.");
    } else {
        println!("These known MIDI devices were found:");
        for d in &midi_devices {
            println!("  - {}", d.device);
        }
    }
    if !prompt_bool("Does this look correct?")? {
        midi_devices = prompt_midi(&midi_inputs, &midi_outputs, Device::all(internal_clocks))?;
    }

    if prompt_bool("Use a color organ?")? {
        let input_port_name = prompt_indexed_value("Input port:", &midi_inputs)?;
        let output_port_name = prompt_indexed_value("Output port:", &midi_outputs)?;
        midi_devices.push(DeviceSpec {
            device: Device::ColorOrgan(ColorOrgan::new(0, 60, 0)?),
            input_port_name,
            output_port_name,
        })
    }

    let controller = Controller::new(args.osc_receive_port, osc_controllers, midi_devices)?;

    let universe_count = patch.universe_count();
    println!("This show requires {universe_count} universe(s).");

    let mut dmx_ports = Vec::new();

    for i in 0..universe_count {
        println!("Assign port to universe {i}:");
        dmx_ports.push(select_port()?);
    }

    if animation_service.is_some() {
        launch_animation_visualizer()?;
    }

    let mut show = Show::new(
        patch,
        args.patch_file,
        controller,
        clocks,
        animation_service,
        args.cli_preview
            .then(Previewer::terminal)
            .unwrap_or_default(),
    )?;

    println!("Running show.");

    show.run(&mut dmx_ports);

    Ok(())
}

fn check_patch(args: CheckArgs) -> Result<()> {
    Patch::from_file(&args.patch_file)?;
    println!("Patch is OK.");
    Ok(())
}

fn prompt_start_animation_service(ctx: &Context) -> Result<Option<AnimationPublisher>> {
    if !prompt_bool("Run animation visualizer?")? {
        return Ok(None);
    }
    Ok(Some(animation_publisher(ctx)?))
}

fn launch_animation_visualizer() -> Result<()> {
    let bin_path = current_exe().context("failed to get the path to the running binary")?;
    std::process::Command::new(bin_path)
        .arg(Command::Viz.to_string())
        .spawn()
        .context("failed to start animation visualizer")?;
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
