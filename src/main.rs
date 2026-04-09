use anyhow::Result;
use clap::Parser;
use log::LevelFilter;
use simplelog::{Config as LogConfig, SimpleLogger};

use crate::cli::Cli;

mod animation;
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
mod gui_state;
mod master;
mod midi;
mod osc;
mod preview;
mod show;
mod show_file;
mod strobe;
mod touchosc;
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

    config_gui::run_console(args.osc_receive_port)
}
