use crate::dmx::DmxAddr;
use crate::fixture::GroupName;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
pub enum DmxAddrConfig {
    /// A contiguous block of fixtures.
    StartAndCount { start: DmxAddr, count: usize },
    /// A single DMX address.
    Single(DmxAddr),
}

#[derive(Clone, Debug, Deserialize)]
pub struct FixtureGroupConfig {
    /// The type of fixture to patch.
    pub fixture: String,
    /// The DMX address configuration to patch this fixture at.
    /// If no address is provided, assume this fixture doesn't need to render.
    #[serde(default)]
    pub addr: Option<DmxAddrConfig>,
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
    /// If true, assign to a channel. Defaults to true.
    #[serde(default = "_true")]
    pub channel: bool,
}

const fn _true() -> bool {
    true
}

impl FixtureGroupConfig {
    pub fn fixture_configs(&self, channel_count: usize) -> Vec<FixtureConfig> {
        let Some(addr_cfg) = self.addr else {
            return vec![FixtureConfig::from_group_config(self, None)];
        };
        match addr_cfg {
            DmxAddrConfig::Single(addr) => {
                vec![FixtureConfig::from_group_config(self, Some(addr))]
            }
            DmxAddrConfig::StartAndCount { start, count } => (0..count)
                .map(|i| FixtureConfig::from_group_config(self, Some(start + (i * channel_count))))
                .collect(),
        }
    }
}

/// A single instance of a fixture to patch, produced by a FixtureGroupConfig.
#[derive(Clone, Debug)]
pub struct FixtureConfig {
    /// The type of fixture to patch.
    pub fixture: String,
    /// The DMX address to patch this fixture at.
    /// If no address it provided, assume this fixture doesn't need to render.
    pub addr: Option<DmxAddr>,
    /// The universe this fixture is patched in.
    pub universe: usize,
    /// True if this fixture's controls should be flipped when running in mirror mode.
    pub mirror: bool,
    /// For fixtures that we may want to separately control multiple instances,
    /// provide a group index.  Most fixtures do not use this.
    pub group: Option<GroupName>,
    /// Additional key-value string options for configuring specific fixture types.
    pub options: Options,
    /// If true, assign to a channel.
    pub channel: bool,
}

impl FixtureConfig {
    fn from_group_config(group: &FixtureGroupConfig, addr: Option<DmxAddr>) -> Self {
        Self {
            fixture: group.fixture.clone(),
            addr,
            universe: group.universe,
            mirror: group.mirror,
            group: group.group.clone(),
            options: group.options.clone(),
            channel: group.channel,
        }
    }
}

pub type Options = HashMap<String, String>;
