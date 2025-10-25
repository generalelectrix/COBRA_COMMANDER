use crate::dmx::DmxAddr;
use anyhow::{bail, ensure, Result};
use itertools::Itertools;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
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
    #[serde(default)]
    pub addr: Option<DmxAddrConfig>,

    /// The universe this fixture is patched in.
    /// Defaults to 0.
    #[serde(default)]
    pub universe: usize,

    /// True if this fixture's controls should be flipped when running in mirror mode.
    #[serde(default)]
    pub mirror: bool,

    /// Additional options for configuring individual fixtures.
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

/// Options that will be passed to a fixture to parse into a strong type.
/// Using Mapping allows us to accept any valid yaml as the keys and values,
/// so fixtures are pretty free to structure their options structs.
#[derive(Clone, Default, Debug, Deserialize)]
pub struct Options {
    #[serde(flatten)]
    value: Mapping,
}

impl Options {
    /// Parse these options as a strong type.
    pub fn parse<T: DeserializeOwned>(self) -> Result<T> {
        Ok(serde_yaml::from_value(Value::Mapping(self.value))?)
    }

    /// Return an error if the options are not empty.
    pub fn ensure_empty(&self) -> Result<()> {
        ensure!(
            self.value.is_empty(),
            "these options were not expected: {}",
            self.value.keys().map(string_value).join(", ")
        );
        Ok(())
    }
}

/// Format a Value as a string, providing simple placeholders for complex types.
fn string_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(s) => s.clone(),
        Value::Sequence(s) => format!("<sequence of length {}>", s.len()),
        Value::Mapping(m) => format!("<mapping of length {}>", m.len()),
        Value::Tagged(_) => "<tagged value>".to_string(),
    }
}

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

#[cfg(test)]
mod test {
    use super::*;
    fn assert_fail_parse(patch_yaml: &str, snippet: &str) {
        let err = serde_yaml::from_str::<Vec<FixtureGroupConfig>>(patch_yaml).unwrap_err();
        assert!(
            format!("{err:#}").contains(snippet),
            "error message didn't contain '{snippet}':\n{err:#}"
        );
    }

    // This is arguably "testing serde" but we want to have some machine-verifiable
    // proof that these things fail.
    #[test]
    fn test_missing_fields() {
        assert_fail_parse("- foobar: Baz", "missing field `fixture`");
        assert_fail_parse("- fixture: Foo", "missing field `patches`");
    }
}
