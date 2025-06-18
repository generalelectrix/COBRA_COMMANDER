use anyhow::Context as _;
use anyhow::{bail, Result};
use clap::Parser;
use clock_service::prompt_start_clock_service;
use fixture::Patch;
use local_ip_address::local_ip;
use log::info;
use log::LevelFilter;
use midi::Device;
use osc::prompt_osc_config;
use osc::GroupControlMap;
use reqwest::Url;
use rust_dmx::select_port;
use show::Clocks;
use simplelog::{Config as LogConfig, SimpleLogger};
use std::fs::File;
use std::path::PathBuf;
use tunnels::audio::prompt_audio;
use tunnels::audio::AudioInput;
use tunnels::clock_bank::ClockBank;
use tunnels::midi::list_ports;
use tunnels::midi::prompt_midi;
use zmq::Context;

use crate::config::FixtureGroupConfig;
use crate::control::Controller;
use crate::show::Show;

mod animation;
mod channel;
mod clock_service;
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

#[derive(Parser, Debug)]
#[command(about)]
struct Args {
    #[arg(long)]
    check_patch: bool,

    #[arg(long, default_value_t = 8000)]
    receive_port: u16,

    #[arg(long)]
    wled_addr: Option<Url>,

    #[arg(long)]
    debug: bool,

    patch: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::try_parse()?;

    let patch = {
        let patch_file = File::open(&args.patch).with_context(|| {
            format!(
                "unable to read patch file \"{}\"",
                args.patch.to_string_lossy()
            )
        })?;
        let fixtures = serde_yaml::from_reader(patch_file)?;
        if args.check_patch {
            return check_patch(fixtures);
        }
        Patch::patch_all(fixtures)?
    };

    let log_level = if args.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };

    SimpleLogger::init(log_level, LogConfig::default())?;

    let audio_device = prompt_audio()?
        .map(|device_name| AudioInput::new(Some(device_name)))
        .transpose()?;

    let clocks = if let Some(clock_service) = prompt_start_clock_service(Context::new())? {
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

    match local_ip() {
        Ok(ip) => info!("Listening for OSC at {}:{}.", ip, args.receive_port),
        Err(e) => info!("Unable to fetch local IP address: {}.", e),
    }

    let osc_controllers = prompt_osc_config(args.receive_port)?.unwrap_or_default();

    let (midi_inputs, midi_outputs) = list_ports()?;
    let midi_devices = prompt_midi(&midi_inputs, &midi_outputs, Device::all())?;
    if osc_controllers.is_empty() && midi_devices.is_empty() {
        bail!("No OSC or midi clients were registered or manually configured.");
    }

    let controller = Controller::new(
        args.receive_port,
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

    let mut show = Show::new(patch, controller, clocks)?;

    show.run(&mut dmx_ports);

    Ok(())
}

fn check_patch(fixtures: Vec<FixtureGroupConfig>) -> Result<()> {
    Patch::patch_all(fixtures)?;
    println!("Patch is OK.");
    Ok(())
}
