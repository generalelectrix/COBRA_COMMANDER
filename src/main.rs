use dmx::DmxAddr;
use local_ip_address::local_ip;
use log::info;
use log::LevelFilter;
use rust_dmx::select_port;
use serde::{Deserialize, Serialize};
use simplelog::{Config as LogConfig, SimpleLogger};
use std::env;
use std::error::Error;
use std::fs::File;

use crate::show::Show;

mod comet;
mod dmx;
mod fixture;
mod lumasphere;
mod osc;
mod show;
mod util;

fn main() -> Result<(), Box<dyn Error>> {
    let config_path = env::args()
        .nth(1)
        .expect("Provide config path as first arg.");
    let cfg = Config::load(&config_path)?;
    let log_level = if cfg.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    SimpleLogger::init(log_level, LogConfig::default())?;

    let ip = local_ip()?;
    info!("Listening for OSC at {}:{}", ip, cfg.receive_port);

    let dmx_port = select_port()?;

    let mut show = Show::new(&cfg)?;

    show.run(dmx_port);

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    receive_port: u16,
    send_host: String,
    send_port: u16,
    dmx_addr: DmxAddr,
    debug: bool,
    fixture: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let config_file = File::open(path)?;
        let cfg: Config = serde_yaml::from_reader(config_file)?;
        Ok(cfg)
    }
}