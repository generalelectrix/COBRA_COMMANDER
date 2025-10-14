use crate::dmx::DmxAddr;
use serde::Deserialize;
use std::{borrow::Borrow, collections::HashMap, fmt::Display, ops::Deref};

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

    /// For fixtures that we may want to separately control multiple instances,
    /// provide a group name.
    #[serde(default)]
    pub group: Option<FixtureGroupKey>,

    /// If true, assign to a channel. Defaults to true.
    #[serde(default = "_true")]
    pub channel: bool,

    /// If true, initialize a color organ for this group.
    #[serde(default)]
    pub color_organ: bool,

    pub patches: Vec<PatchBlock>,

    /// Additional fixture-specific key-value string options for configuring the group.
    /// Any additional keys will be parsed into here.
    #[serde(flatten)]
    pub options: Options,
}

/// One or more instances of a fixture to patch in the context of a group.
#[derive(Clone, Debug, Deserialize)]
pub struct PatchBlock {
    /// The DMX address(es) to patch at, either a single address or a start/count.
    pub addr: Option<DmxAddrConfig>,

    /// The universe this fixture is patched in.
    /// Defaults to 0.
    #[serde(default)]
    pub universe: usize,

    /// True if this fixture's controls should be flipped when running in mirror mode.
    #[serde(default)]
    pub mirror: bool,

    /// Additional key-value string options for configuring individual fixtures.
    #[serde(flatten)]
    pub options: Options,
}

impl PatchBlock {
    /// Return the starting DMX address for this patch block and the number of fixtures in it.
    pub fn start_count(&self) -> (Option<DmxAddr>, usize) {
        let Some(addr) = self.addr else {
            return (None, 1);
        };
        match addr {
            DmxAddrConfig::Single(addr) => (Some(addr), 1),
            DmxAddrConfig::StartAndCount { start, count } => (Some(start), count),
        }
    }
}

const fn _true() -> bool {
    true
}

pub type Options = HashMap<String, String>;

/// Uniquely identify a specific fixture group.
#[derive(Clone, PartialEq, Eq, Hash, Deserialize, Debug)]
pub struct FixtureGroupKey(pub String);

impl Display for FixtureGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Borrow<str> for FixtureGroupKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Deref for FixtureGroupKey {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
