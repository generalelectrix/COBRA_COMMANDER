use crate::dmx::DmxAddr;
use anyhow::{Result, ensure};
use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};
use std::{borrow::Borrow, fmt::Display, ops::Deref};
use uuid::Uuid;

/// Stable, opaque identifier for a fixture group.
///
/// Minted when the group is first created in the patch editor and preserved
/// across renames, repatches, and (eventually) restarts. The operator never
/// sees this; it exists purely so the controller can answer "is this the same
/// group it was before?" when the display name has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(Uuid);

impl GroupId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for GroupId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DmxAddrConfig {
    /// A contiguous block of fixtures.
    StartAndCount { start: DmxAddr, count: usize },
    /// A single DMX address.
    Single(DmxAddr),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixtureGroupConfig {
    /// Stable opaque identifier. Minted on first load (or first creation in the
    /// patch editor) and preserved across renames so the controller can carry
    /// per-group state forward through repatches.
    #[serde(default)]
    pub id: GroupId,

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

impl FixtureGroupConfig {
    /// Get the key for this group, either the name of the fixture, or the
    /// group name if one is provided.
    pub fn key(&self) -> &str {
        self.group.as_deref().unwrap_or(&self.fixture)
    }
}

/// One or more instances of a fixture to patch in the context of a group.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Options {
    #[serde(flatten)]
    value: Mapping,
}

impl Options {
    /// Parse these options as a strong type.
    pub fn parse<T: DeserializeOwned>(self) -> Result<T> {
        Ok(serde_yaml::from_value(Value::Mapping(self.value))?)
    }

    /// Build Options programmatically from key-value pairs.
    pub fn from_entries(entries: impl IntoIterator<Item = (String, serde_yaml::Value)>) -> Self {
        let mut mapping = serde_yaml::Mapping::new();
        for (key, value) in entries {
            mapping.insert(serde_yaml::Value::String(key), value);
        }
        Self { value: mapping }
    }

    /// Get a string value by key.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.value
            .get(Value::String(key.to_string()))
            .map(string_value)
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

#[cfg(test)]
impl Options {
    /// Set a string value by key.
    pub fn set_string(&mut self, key: &str, val: &str) {
        self.value.insert(
            Value::String(key.to_string()),
            Value::String(val.to_string()),
        );
    }

    /// Get a bool value by key.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.value
            .get(Value::String(key.to_string()))
            .and_then(|v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            })
    }

    /// Set a bool value by key.
    pub fn set_bool(&mut self, key: &str, val: bool) {
        self.value
            .insert(Value::String(key.to_string()), Value::Bool(val));
    }

    /// Remove a key from the options.
    pub fn remove(&mut self, key: &str) {
        self.value.remove(Value::String(key.to_string()));
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
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
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

    #[test]
    fn options_string_round_trip() {
        let mut opts = Options::default();
        assert_eq!(opts.get_string("foo"), None);
        opts.set_string("foo", "bar");
        assert_eq!(opts.get_string("foo").as_deref(), Some("bar"));
    }

    #[test]
    fn options_bool_round_trip() {
        let mut opts = Options::default();
        assert_eq!(opts.get_bool("flag"), None);
        opts.set_bool("flag", true);
        assert_eq!(opts.get_bool("flag"), Some(true));
        opts.set_bool("flag", false);
        assert_eq!(opts.get_bool("flag"), Some(false));
    }

    #[test]
    fn options_remove() {
        let mut opts = Options::default();
        opts.set_string("key", "value");
        assert!(opts.get_string("key").is_some());
        opts.remove("key");
        assert_eq!(opts.get_string("key"), None);
    }

    #[test]
    fn options_remove_missing_key_is_noop() {
        let mut opts = Options::default();
        opts.remove("nonexistent"); // should not panic
    }

    /// Existing patch YAML on disk does not carry a `id` field. Loading must
    /// silently mint a UUID per group; round-tripping that loaded config back
    /// to YAML and reloading it must preserve the freshly-minted id so identity
    /// is stable on the next launch.
    #[test]
    fn fixture_group_config_serde_defaults_id_and_round_trips() {
        let yaml = "
- fixture: Color
  control_color_space: Hsluv
  patches:
    - addr: 1
- fixture: Dimmer
  group: TestGroup
  patches:
    - addr: 1
      universe: 1
";
        let loaded: Vec<FixtureGroupConfig> =
            serde_yaml::from_str(yaml).expect("legacy YAML without ids should still parse");
        assert_eq!(loaded.len(), 2);

        let id_color = loaded[0].id;
        let id_dimmer = loaded[1].id;
        assert_ne!(id_color, id_dimmer, "each group gets a distinct fresh id");

        let round_tripped: Vec<FixtureGroupConfig> =
            serde_yaml::from_str(&serde_yaml::to_string(&loaded).unwrap()).unwrap();
        assert_eq!(round_tripped[0].id, id_color);
        assert_eq!(round_tripped[1].id, id_dimmer);
    }
}
