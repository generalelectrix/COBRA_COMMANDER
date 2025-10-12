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
use osc::GroupControlMap;
use reqwest::Url;
use rust_dmx::select_port;
use simplelog::{Config as LogConfig, SimpleLogger};
use std::env::current_exe;
use std::fs::File;
use std::path::PathBuf;
use strum_macros::Display;
use tunnels::audio::prompt_audio;
use tunnels::audio::AudioInput;
use tunnels::clock_bank::ClockBank;
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
mod show;
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

    /// Run the animation visualizer.
    Viz,
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

    /// URL to use to communicate with a WLED instance.
    #[arg(long)]
    wled_addr: Option<Url>,
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
        Command::Viz => run_animation_visualizer(),
    }
}

fn run_show(args: RunArgs) -> Result<()> {
    let patch = {
        let patch_file = File::open(&args.patch_file).with_context(|| {
            format!(
                "unable to read patch file \"{}\"",
                args.patch_file.to_string_lossy()
            )
        })?;
        let fixtures = serde_yaml::from_reader(patch_file)?;
        if args.check_patch {
            return check_patch(fixtures);
        }
        Patch::patch_all(fixtures)?
    };

    let audio_device = prompt_audio()?
        .map(|device_name| AudioInput::new(Some(device_name)))
        .transpose()?;

    let zmq_ctx = Context::new();

    let clocks = if let Some(clock_service) = prompt_start_clock_service(zmq_ctx.clone())? {
        if let Some(audio_input) = audio_device {
            let mut audio_controls = GroupControlMap::default();
            crate::osc::audio::map_controls(&mut audio_controls);
            // Local audio input, remote clocks.
            Clocks::Mixed {
                service: clock_service,
                audio_input,
                audio_controls,
            }
        } else {
            Clocks::Service(clock_service)
        }
    } else {
        let clocks = ClockBank::default();
        let mut audio_controls = GroupControlMap::default();
        crate::osc::audio::map_controls(&mut audio_controls);
        Clocks::Internal {
            clocks,
            audio_input: audio_device.unwrap_or_else(|| AudioInput::new(None).unwrap()),
            audio_controls,
        }
    };

    let animation_service = prompt_start_animation_service(&zmq_ctx)?;

    match local_ip() {
        Ok(ip) => println!("Listening for OSC at {}:{}.", ip, args.osc_receive_port),
        Err(e) => println!("Unable to fetch local IP address: {e}."),
    }

    let osc_controllers = prompt_osc_config(args.osc_receive_port)?.unwrap_or_default();

    let (midi_inputs, midi_outputs) = list_ports()?;
    let mut midi_devices = prompt_midi(&midi_inputs, &midi_outputs, Device::all())?;

    for d in &midi_devices {
        if matches!(d.device, Device::CmdMM1(_)) && !matches!(clocks, Clocks::Internal { .. }) {
            bail!("Configured a CMD MM-1 but the clock service is active; do not activate the clock service if you want local clock controls.");
        }
    }

    if osc_controllers.is_empty() && midi_devices.is_empty() {
        bail!("No OSC or midi clients were registered or manually configured.");
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

    let controller = Controller::new(
        args.osc_receive_port,
        osc_controllers,
        midi_devices,
        args.wled_addr,
    )?;

    let universe_count = patch.universe_count();
    println!("This show requires {universe_count} universes.");

    let mut dmx_ports = Vec::new();

    for i in 0..universe_count {
        println!("Assign port to universe {i}:");
        dmx_ports.push(select_port()?);
    }

    if animation_service.is_some() {
        launch_animation_visualizer()?;
    }

    let mut show = Show::new(patch, controller, clocks, animation_service)?;

    println!("Running show.");

    show.run(&mut dmx_ports);

    Ok(())
}

fn check_patch(fixtures: Vec<FixtureGroupConfig>) -> Result<()> {
    Patch::patch_all(fixtures)?;
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
