use crate::dmx::DmxAddr;
use crate::fixture::GroupName;
use crate::osc::OscClientId;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_receive_port")]
    pub receive_port: u16,
    #[serde(default)]
    pub controllers: Vec<OscClientId>,
    #[serde(default)]
    pub debug: bool,
    pub fixtures: Vec<FixtureConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnimationGroup {
    pub fixture_type: String,
    pub group: GroupName,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let config_file = File::open(path)?;
        let cfg: Config = serde_yaml::from_reader(config_file)?;
        Ok(cfg)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct FixtureConfig {
    pub name: String,
    pub addr: DmxAddr,
    /// The universe this fixture is patched in.
    /// Defaults to 0.
    #[serde(default)]
    pub universe: usize,
    /// True if this fixture's controls should be flipped when running in mirror mode.
    #[serde(default)]
    pub mirror: bool,
    /// For fixtures that we may want to separately control multiple instances,
    /// provide a group index.  Most fixtures do not use this.
    #[serde(default)]
    pub group: Option<GroupName>,
    /// Additional key-value string options for configuring specific fixture types.
    #[serde(default)]
    pub options: Options,
    /// If true, assign to a channel.
    #[serde(default)]
    pub channel: bool,
}

pub type Options = HashMap<String, String>;

const fn default_receive_port() -> u16 {
    8000
}
