//! Types and traits related to patching fixtures.
use anyhow::{Context, Result, anyhow, bail, ensure};
use itertools::Itertools;
use log::info;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use super::fixture::FixtureType;
use super::group::FixtureGroup;
use crate::config::{FixtureGroupConfig, GroupId, GroupName};
use crate::dmx::UniverseIdx;
use crate::fixture::group::GroupFixtureConfig;
use crate::positioner::PositionerPresets;
use crate::show_file::ShowPatchConfigs;

mod option;
mod patcher;

pub use patcher::{
    CreateAnimatedGroup, CreateNonAnimatedGroup, PATCHERS, PatchConfig, PatchFixture, Patcher,
};

pub use option::{
    AsPatchOption, NoOptions, OptionsMenu, PatchOption, deserialize_bipolar, enum_patch_option,
};

/// Build an [`anyhow::Error`] describing an internal patch inconsistency.
///
/// Use this for error paths that can only be reached due to a programmer bug
/// — a violated patch invariant rather than a user-input or config error. We
/// surface these as ordinary errors (rather than panicking) so a show in
/// progress can keep running, but the message format makes the diagnosis
/// unambiguous and the unique code makes the originating site easy to grep.
///
/// `code` should be of the form `"PI-NNN"` and unique across the codebase.
/// Pass a `format!(...)`-built `String` (or any `Display`) for `details` to
/// include the local context (channel ids, group names, etc.) that made the
/// invariant violation visible.
pub fn patch_inconsistency(code: &'static str, details: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(
        "Error code: {code}. Internal patch inconsistency: {details}. \
         This is a bug — please report to this application's developers."
    )
}

/// Where a fixture group physically lives inside a `Patch`.
///
/// Returned by name/id lookups so callers can both access the group and know
/// whether it's bound to a channel. Locations are valid for the lifetime of
/// the `Patch` they came from; a repatch builds a fresh `Patch` and invalidates
/// any locations from the previous one.
///
/// In short - get these and use them immediately, don't store them anywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLocation {
    /// Channel-bound; index is the channel id (= position in `Patch::channels`).
    Channel(ChannelId),
    /// Not bound to a channel; index is the position in `Patch::non_channel`.
    NonChannel(usize),
}

impl GroupLocation {
    /// Return the channel id if this group is channel-bound, else `None`.
    pub fn as_channel(&self) -> Option<ChannelId> {
        match self {
            Self::Channel(c) => Some(*c),
            Self::NonChannel(_) => None,
        }
    }
}

/// Factory for fixture instances.
///
/// Owns the fixture groups themselves and the indices used to look them up by
/// group name, stable id, or channel position. Maintains a mapping of which
/// DMX addresses are in use by which fixture, to prevent addressing collisions.
///
/// Storage layout: groups physically live in either `channels` (in channel
/// order) or `non_channel` (not channel-bound). The `by_id` and `by_name`
/// maps hold `GroupLocation`s that point into one of those two `Vec`s. This
/// gives O(1) lookup by id, name, and channel position; iteration over all
/// groups chains the two `Vec`s.
pub struct Patch {
    /// Map of registered patchers.
    patchers: HashMap<String, Patcher>,
    /// Channel-bound groups in channel order. Position in this `Vec` IS the
    /// `ChannelId`.
    channels: Vec<FixtureGroup>,
    /// Groups that are not bound to a channel (controlled only via OSC by name).
    non_channel: Vec<FixtureGroup>,
    /// O(1) lookup from stable id to physical location.
    by_id: HashMap<GroupId, GroupLocation>,
    /// O(1) lookup from group name to physical location.
    by_name: HashMap<GroupName, GroupLocation>,
    /// DMX address allocations, indexed by `(universe, dmx_index)`.
    used_addrs: UsedAddrs,
    /// The configs that built this patch.
    configs: ShowPatchConfigs,
}

impl Patch {
    /// Return the full menu of fixtures we can patch, sorted by name.
    pub fn menu() -> Vec<Patcher> {
        PATCHERS.iter().cloned().sorted_by_key(|p| p.name).collect()
    }

    /// Initialize a new fixture patch.
    fn new() -> Self {
        assert!(!PATCHERS.is_empty());
        let mut patchers = HashMap::new();
        for patcher in PATCHERS {
            let name = patcher.name.to_string();
            assert!(
                !patchers.contains_key(&name),
                "duplicate patcher registered for {}",
                patcher.name
            );
            patchers.insert(name, patcher.clone());
        }
        Self {
            patchers,
            channels: Vec::new(),
            non_channel: Vec::new(),
            by_id: HashMap::new(),
            by_name: HashMap::new(),
            used_addrs: Default::default(),
            configs: ShowPatchConfigs::default(),
        }
    }

    /// Get the patcher for a fixture type.
    fn patcher(&self, name: &str) -> Result<Patcher> {
        let Some(p) = self.patchers.get(name).cloned() else {
            bail!("unknown fixture type \"{name}\"");
        };
        Ok(p)
    }

    /// Initialize a patch from a collection of groups.
    pub fn patch_all(groups: ShowPatchConfigs) -> Result<Self> {
        let mut patch = Self::new();
        for group in groups.iter() {
            patch.patch(group).with_context(|| {
                format!(
                    "patching {}{}",
                    group.fixture,
                    group
                        .group
                        .as_ref()
                        .map(|g| format!("({g})"))
                        .unwrap_or_default()
                )
            })?;
        }
        patch.configs = groups;
        Ok(patch)
    }

    /// Re-initialize a patch from new configs.
    ///
    /// Groups whose stable `GroupId` matches one from the previous patch (and
    /// whose fixture type and options are compatible) keep their live runtime
    /// state — animation values, strobe state, etc. — via
    /// [`FixtureGroup::reconfigure_from`]. Groups that don't match start fresh.
    ///
    /// If any patch error occurs, the original patch remains unchanged.
    ///
    /// TODO: this approach will become problematic if we add control for any
    /// fixtures that require exclusive control of an external resource such as
    /// binding a socket.
    pub fn repatch(&mut self, groups: ShowPatchConfigs) -> Result<()> {
        let mut new_patch = Self::patch_all(Arc::clone(&groups))?;
        // Drain old groups into an id-keyed map for O(1) reconciliation lookup.
        let mut old_by_id: HashMap<GroupId, FixtureGroup> = std::mem::take(&mut self.channels)
            .into_iter()
            .chain(std::mem::take(&mut self.non_channel))
            .map(|g| (g.id(), g))
            .collect();
        for group in new_patch.iter_mut() {
            if let Some(old) = old_by_id.remove(&group.id()) {
                group.reconfigure_from(old);
            }
        }
        new_patch.configs = groups;
        *self = new_patch;
        Ok(())
    }

    /// A cheap-clone handle to the configs that built this patch.
    pub fn configs(&self) -> ShowPatchConfigs {
        Arc::clone(&self.configs)
    }

    /// Build a patch from a loaded show file, applying its positioner state
    /// to the patched groups.
    pub fn from_show_file(show_file: crate::show_file::ShowFile) -> Result<Self> {
        let mut patch = Self::patch_all(show_file.patch)?;
        patch.apply_loaded_positioners(show_file.positioners);
        Ok(patch)
    }

    /// Install loaded positioner presets on the patched groups. Each entry
    /// is reconciled to the group's current fixture count before
    /// installation. Entries for unknown or non-positionable groups are
    /// logged and dropped.
    fn apply_loaded_positioners(&mut self, positioners: HashMap<GroupId, PositionerPresets>) {
        for (id, presets) in positioners {
            let Some(location) = self.by_id.get(&id).copied() else {
                log::warn!("Loaded positioner for unknown group {id:?}; dropping");
                continue;
            };
            let group = match location {
                GroupLocation::Channel(c) => self.channels.get_mut(c.inner()),
                GroupLocation::NonChannel(i) => self.non_channel.get_mut(i),
            };
            let Some(group) = group else {
                log::error!(
                    "internal: by_id pointed at missing slot for group {id:?}; dropping positioner"
                );
                continue;
            };
            if let Err(e) = group.install_positioner_presets(presets) {
                log::warn!("Loaded positioner for group {id:?}: {e:#}; dropping");
            }
        }
    }

    /// Patch a single fixture group config.
    fn patch(&mut self, cfg: &FixtureGroupConfig) -> Result<()> {
        let patcher = self.patcher(&cfg.fixture)?;

        if let Some(group_name) = &cfg.group {
            // If there's a patcher that matches this group name, fail.
            ensure!(
                self.patcher(group_name).is_err(),
                "the group name '{group_name}' cannot be used because it is also a fixture name"
            );
        }

        let group_name = GroupName(cfg.name().to_string());

        ensure!(
            !self.by_name.contains_key(&group_name),
            "duplicate group name '{group_name}'"
        );
        ensure!(
            !self.by_id.contains_key(&cfg.id),
            "duplicate group id '{:?}' for group '{group_name}'",
            cfg.id
        );

        let mut group = (patcher.create_group)(cfg.id, group_name.clone(), cfg.options.clone())?;

        ensure!(!cfg.patches.is_empty(), "no patches specified");

        for block in cfg.patches.iter() {
            let (start_addr, count) = block.start_count();

            let patch_cfg = (patcher.create_patch)(cfg.options.clone(), block.options.clone())?;

            match start_addr {
                None => {
                    ensure!(
                        patch_cfg.channel_count == 0,
                        "no DMX address provided for a fixture that requests {} DMX channel(s)",
                        patch_cfg.channel_count
                    );
                    // Should be true by type construction in the configuration.
                    assert_eq!(count, 1);
                    info!(
                        "Controlling {} (non-DMX fixture) (group: {}).",
                        cfg.fixture,
                        cfg.group.as_deref().unwrap_or("none")
                    );
                    group.patch(GroupFixtureConfig {
                        dmx_index: None,
                        universe: block.universe,
                        channel_count: patch_cfg.channel_count,
                        mirror: block.mirror,
                        render_mode: patch_cfg.render_mode,
                    });
                }
                Some(mut dmx_addr) => {
                    ensure!(
                        patch_cfg.channel_count > 0,
                        "DMX start address {dmx_addr} provided for a fixture that is not DMX-controlled"
                    );
                    dmx_addr.validate()?;
                    for _ in 0..count {
                        let fixture_cfg = GroupFixtureConfig {
                            dmx_index: Some(dmx_addr.dmx_index()),
                            universe: block.universe,
                            channel_count: patch_cfg.channel_count,
                            mirror: block.mirror,
                            render_mode: patch_cfg.render_mode,
                        };

                        if let Some(dmx_index) = fixture_cfg.dmx_index {
                            self.used_addrs.allocate(
                                patcher.name,
                                fixture_cfg.universe,
                                dmx_index,
                                fixture_cfg.channel_count,
                            )?;
                        };

                        info!(
                            "Controlling {} at {} (group: {}).",
                            cfg.fixture,
                            dmx_addr,
                            cfg.group.as_deref().unwrap_or("none")
                        );

                        group.patch(fixture_cfg);

                        dmx_addr = dmx_addr + patch_cfg.channel_count;
                    }
                }
            }
        }

        if cfg.color_organ {
            group.use_color_organ();
        }
        group.init_positioner_if_supported();

        let id = group.id();
        let location = if cfg.channel {
            let channel_id = ChannelId(self.channels.len());
            self.channels.push(group);
            GroupLocation::Channel(channel_id)
        } else {
            let idx = self.non_channel.len();
            self.non_channel.push(group);
            GroupLocation::NonChannel(idx)
        };
        self.by_id.insert(id, location);
        self.by_name.insert(group_name, location);
        Ok(())
    }

    /// Dynamically get the universe count.
    ///
    /// This is just based on the indices provided by fixtures; we could have
    /// "holes" where we don't actually have any fixtures patched.
    pub fn universe_count(&self) -> usize {
        self.iter()
            .flat_map(|group| group.fixture_configs())
            .map(|cfg| cfg.universe)
            .max()
            .unwrap_or_default()
            + 1
    }

    // ---- Lookups ------------------------------------------------------------

    /// Return the first channel ID, if we have at least one group with a channel.
    pub fn first_channel(&self) -> Option<ChannelId> {
        (self.channel_count() > 0).then_some(ChannelId(0))
    }

    /// Look up a group by its name. Exercised by tests; production OSC
    /// dispatch goes through [`lookup_mut_by_name`] which also returns the
    /// channel id.
    #[cfg(test)]
    pub fn group_by_name(&self, name: &str) -> Option<&FixtureGroup> {
        match *self.by_name.get(name)? {
            GroupLocation::Channel(c) => self.channels.get(c.inner()),
            GroupLocation::NonChannel(i) => self.non_channel.get(i),
        }
    }

    /// Look up a group by its name, also returning the channel id if it's
    /// channel-bound. Hot path for OSC dispatch.
    pub fn lookup_mut_by_name(
        &mut self,
        name: &str,
    ) -> Result<(&mut FixtureGroup, Option<ChannelId>)> {
        let location = *self
            .by_name
            .get(name)
            .ok_or_else(|| anyhow!("fixture {name} not found in patch"))?;
        let channel_id = location.as_channel();
        let group = match location {
            GroupLocation::Channel(c) => self.channels.get_mut(c.inner()),
            GroupLocation::NonChannel(i) => self.non_channel.get_mut(i),
        }
        .ok_or_else(|| {
            patch_inconsistency(
                "PI-001",
                format!("by_name had {location:?} for '{name}' but the backing vec lookup failed"),
            )
        })?;

        Ok((group, channel_id))
    }

    /// Look up the channel id for a group by stable id, or `None` if the group
    /// isn't channel-bound (or doesn't exist).
    pub fn channel_for_id(&self, id: GroupId) -> Option<ChannelId> {
        self.by_id.get(&id)?.as_channel()
    }

    // ---- Channel queries ----------------------------------------------------

    /// Number of channels in this patch.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Validate a raw channel index (typically from an OSC payload) and return
    /// the matching channel id paired with the group on that channel.
    pub fn channel(&self, raw_id: usize) -> Result<(ChannelId, &FixtureGroup)> {
        let group = self.channels.get(raw_id).ok_or_else(|| {
            anyhow!(
                "channel selector {raw_id} out of range, only {} channels are configured",
                self.channels.len()
            )
        })?;
        Ok((ChannelId(raw_id), group))
    }

    /// Mutable counterpart to [`Patch::channel`].
    pub fn channel_mut(&mut self, raw_id: usize) -> Result<(ChannelId, &mut FixtureGroup)> {
        let len = self.channels.len();
        let group = self.channels.get_mut(raw_id).ok_or_else(|| {
            anyhow!("channel selector {raw_id} out of range, only {len} channels are configured")
        })?;
        Ok((ChannelId(raw_id), group))
    }

    /// Look up the group on a specific channel by trusted [`ChannelId`].
    ///
    /// A `ChannelId` can only be produced by methods on this `Patch` (either
    /// [`channel`], [`channels_with_ids`], or [`channel_ids`]), so failure
    /// here indicates a programmer bug rather than an invalid input — the
    /// error is reported via [`patch_inconsistency`] so the show can keep
    /// running.
    pub fn channel_group(&self, channel: ChannelId) -> Result<&FixtureGroup> {
        self.channels.get(channel.inner()).ok_or_else(|| {
            patch_inconsistency(
                "PI-002",
                format!(
                    "channel {channel} has no group in patch (channel_count = {})",
                    self.channels.len()
                ),
            )
        })
    }

    /// Mutable counterpart to [`Patch::channel_group`].
    pub fn channel_group_mut(&mut self, channel: ChannelId) -> Result<&mut FixtureGroup> {
        let len = self.channels.len();
        self.channels.get_mut(channel.inner()).ok_or_else(|| {
            patch_inconsistency(
                "PI-003",
                format!("channel {channel} has no group in patch (channel_count = {len})"),
            )
        })
    }

    /// Iterate over `(ChannelId, &FixtureGroup)` pairs in channel order.
    pub fn channels_with_ids(&self) -> impl Iterator<Item = (ChannelId, &FixtureGroup)> + '_ {
        self.channels
            .iter()
            .enumerate()
            .map(|(i, g)| (ChannelId(i), g))
    }

    /// Iterate over the channel labels, in channel order. Uses each group's
    /// qualified name.
    pub fn channel_labels(&self) -> impl Iterator<Item = String> + '_ {
        self.channels.iter().map(|g| g.qualified_name().to_string())
    }

    // ---- All-groups iteration ----------------------------------------------

    /// Iterate over all patched fixture groups (channel-bound first, then
    /// non-channel).
    pub fn iter(&self) -> impl Iterator<Item = &FixtureGroup> {
        self.channels.iter().chain(self.non_channel.iter())
    }

    /// Iterate over all patched fixture groups, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FixtureGroup> {
        self.channels.iter_mut().chain(self.non_channel.iter_mut())
    }
}

/// Mapping between a universe/address pair and the type of fixture already
/// addressed over that pair, as well as the starting address.
#[derive(Default, Clone)]
struct UsedAddrs(HashMap<(UniverseIdx, usize), (FixtureType, usize)>);

impl UsedAddrs {
    /// Attempt to allocate requested addresses for the provided fixture type.
    ///
    /// The addresses will only be allocated if there are no conflicts.
    ///
    /// Fixture types with "mutual affinity" are allowed to patch over each other;
    /// this is a hack to allow representing a single fixture type as multiple
    /// independent groups. Take care not to fuck this up.
    pub fn allocate(
        &mut self,
        fixture_type: FixtureType,
        universe: UniverseIdx,
        start_dmx_index: usize,
        channel_count: usize,
    ) -> Result<()> {
        ensure!(
            start_dmx_index < 512,
            "dmx address {} out of range",
            start_dmx_index + 1
        );
        let next_dmx_addr = start_dmx_index + channel_count;
        ensure!(
            next_dmx_addr <= 512,
            "impossible to patch a fixture with {channel_count} channels at start address {}",
            start_dmx_index + 1
        );
        for this_index in start_dmx_index..start_dmx_index + channel_count {
            if let Some((existing_fixture, patched_at)) = self.0.get(&(universe, this_index)) {
                if Self::have_mutual_affinity(fixture_type, *existing_fixture) {
                    continue;
                }
                bail!(
                    "{fixture_type} at {} overlaps at DMX address {} in universe {} with {} at {}",
                    start_dmx_index + 1,
                    this_index + 1,
                    universe,
                    existing_fixture,
                    patched_at + 1,
                );
            }
        }
        // No conflicts; allocate addresses.
        for this_index in start_dmx_index..start_dmx_index + channel_count {
            let existing = self
                .0
                .insert((universe, this_index), (fixture_type, start_dmx_index));
            debug_assert!(match existing {
                None => true,
                Some((existing, _)) => Self::have_mutual_affinity(existing, fixture_type),
            });
        }

        Ok(())
    }

    fn have_mutual_affinity(f0: FixtureType, f1: FixtureType) -> bool {
        // TODO: destroy this or generalize it.
        let swarm = crate::fixture::swarmolon::affinity();
        swarm.contains(&f0) && swarm.contains(&f1)
    }
}

/// The index of a channel within the patch's channel-bound groups.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Deserialize)]
pub struct ChannelId(usize);

impl ChannelId {
    pub fn inner(&self) -> usize {
        self.0
    }

    /// Construct a `ChannelId` directly from a raw index. For tests only —
    /// production code receives `ChannelId`s from `Patch` lookups so they're
    /// guaranteed to refer to a real channel.
    #[cfg(test)]
    pub fn for_test(raw: usize) -> Self {
        Self(raw)
    }
}

impl From<ChannelId> for usize {
    fn from(value: ChannelId) -> Self {
        value.0
    }
}

impl Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        channel::mock::no_op_emitter,
        config::{FixtureGroupConfig, Options},
        dmx::DmxBuffer,
        fixture::{
            color::Model as ColorModel,
            control::{OscControlDescription, OscControlType},
            fixture::EnumRenderModel,
        },
    };
    use anyhow::Result;
    use number::UnipolarFloat;

    fn parse(patch_yaml: &str) -> Result<Vec<FixtureGroupConfig>> {
        Ok(serde_yaml::from_str(patch_yaml)?)
    }

    #[test]
    fn test_patcher_display() {
        for p in Patch::menu() {
            println!("{p}");
        }
    }

    #[test]
    fn test_ok() -> Result<()> {
        let cfg = parse(
            "
- fixture: Color
  control_color_space: Hsluv
  patches:
    - addr: 1
    - addr:
        start: 4
        count: 2
      kind: DimmerRgb
- fixture: Dimmer
  group: TestGroup
  channel: false
  patches:
    - addr: 1
      universe: 1
      mirror: true
    - addr: 12
        ",
        )?;
        assert_eq!(2, cfg.len());
        let p = Patch::patch_all(cfg.into())?;
        let channel_pairs: Vec<_> = p.channels_with_ids().collect();
        assert_eq!(channel_pairs.len(), 1);
        assert_eq!("Color", channel_pairs[0].1.qualified_name().to_string());
        assert_eq!(2, p.iter().count());
        let color_configs = p
            .group_by_name("Color")
            .ok_or_else(|| anyhow!("Color group missing"))?
            .fixture_configs();
        assert_eq!(3, color_configs.len());
        assert_eq!(
            color_configs[0],
            GroupFixtureConfig {
                dmx_index: Some(0),
                universe: 0,
                channel_count: 3,
                mirror: false,
                render_mode: Some(ColorModel::Rgb.render_mode()),
            }
        );
        assert_eq!(
            color_configs[2],
            GroupFixtureConfig {
                dmx_index: Some(7),
                universe: 0,
                channel_count: 4,
                mirror: false,
                render_mode: Some(ColorModel::DimmerRgb.render_mode()),
            }
        );
        let dimmer_configs = p
            .group_by_name("TestGroup")
            .ok_or_else(|| anyhow!("TestGroup missing"))?
            .fixture_configs();
        assert_eq!(2, dimmer_configs.len());
        assert_eq!(
            dimmer_configs[0],
            GroupFixtureConfig {
                dmx_index: Some(0),
                universe: 1,
                channel_count: 1,
                mirror: true,
                render_mode: None,
            }
        );
        assert_eq!(
            dimmer_configs[1],
            GroupFixtureConfig {
                dmx_index: Some(11),
                universe: 0,
                channel_count: 1,
                mirror: false,
                render_mode: None,
            }
        );
        Ok(())
    }

    fn assert_fail_patch(patch_yaml: &str, snippet: &str) {
        let Err(err) = Patch::patch_all(
            serde_yaml::from_str::<Vec<FixtureGroupConfig>>(patch_yaml)
                .expect("invalid patch format")
                .into(),
        ) else {
            panic!("patch didn't fail")
        };
        assert!(
            format!("{err:#}").contains(snippet),
            "error message didn't contain '{snippet}':\n{err:#}"
        );
    }

    #[test]
    fn test_collision() {
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 1
    - addr: 1",
            "Dimmer at 1 overlaps at DMX address 1 in universe 0 with Dimmer at 1",
        );
        assert_fail_patch(
            "
- fixture: Color
  patches:
    - addr: 1
    - addr: 3",
            "Color at 3 overlaps at DMX address 3 in universe 0 with Color at 1",
        );
        assert_fail_patch(
            "
- fixture: Color
  patches:
    - addr: 1
- fixture: Dimmer
  patches:
    - addr: 2",
            "Dimmer at 2 overlaps at DMX address 2 in universe 0 with Color at 1",
        );
    }

    #[test]
    fn test_end_of_universe() {
        assert_fail_patch(
            "
- fixture: Color
  patches:
    - addr: 511",
            "impossible to patch a fixture with 3 channels at start address 511",
        );
    }

    #[test]
    fn test_unused_options() {
        assert_fail_patch(
            "
- fixture: Dimmer
  foobar: unused
  patches:
    - addr: 1",
            "group options: these options were not expected: foobar",
        );

        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 1
      foobar: unused",
            "patch options: these options were not expected: foobar",
        );
    }

    #[test]
    fn test_missing_dmx_addr() {
        assert_fail_patch(
            "
- fixture: Color
  patches:
    - kind: Rgbw",
            "no DMX address provided for a fixture that requests 4 DMX channel(s)",
        );
    }

    #[test]
    fn test_dupe_group_name() {
        // Can't specify the same fixture twice with no group.
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 1
- fixture: Dimmer
  patches:
    - addr: 2",
            "duplicate group name 'Dimmer'",
        );
        // Can't use the same group name twice.
        assert_fail_patch(
            "
- fixture: Dimmer
  group: Foo
  patches:
    - addr: 1
- fixture: Dimmer
  group: Foo
  patches:
    - addr: 2",
            "duplicate group name 'Foo'",
        );
    }

    #[test]
    fn test_no_aliasing_fixture() {
        // Can't use a group name that collides with a fixture name.
        assert_fail_patch(
            "
- fixture: Dimmer
  group: Color
  patches:
    - addr: 1",
            "the group name 'Color' cannot be used because it is also a fixture name",
        );
    }

    #[test]
    fn test_no_patches() {
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:",
            "no patches specified",
        );
    }

    #[test]
    fn test_bad_addrs() {
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 0",
            "invalid DMX address 0",
        );
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 513",
            "invalid DMX address 513",
        );
    }

    /// Positioner state is per-group, survives a rename-preserving repatch,
    /// and reconciles its per-fixture offset vectors when the patched
    /// fixture count changes — preserving values where the indices overlap,
    /// padding zeroes when the group grows, and dropping tail entries when
    /// it shrinks.
    #[test]
    fn test_repatch_positioner_reconciles_fixture_count() -> Result<()> {
        use number::BipolarFloat;

        let cfg: Vec<FixtureGroupConfig> = parse(
            "
- fixture: IWashLed
  patches:
    - addr: 1
    - addr: 13
    - addr: 25
    - addr: 37
",
        )?;
        let mut patch = Patch::patch_all(cfg.clone().into())?;

        // Set distinct offsets in slot 0 so we can verify they survive the
        // repatch. Use the second slot too to make sure reconciliation
        // applies across all 8 preset slots, not just the active one.
        {
            let group = patch.iter_mut().next().expect("one group patched");
            let positioner = group
                .positioner_mut()
                .expect("iWashLed opts into the positioner");
            let presets = positioner.presets_mut();
            presets[0].offsets[0].x = BipolarFloat::new(0.1);
            presets[0].offsets[1].y = BipolarFloat::new(-0.2);
            presets[0].offsets[2].x = BipolarFloat::new(0.3);
            presets[0].offsets[3].y = BipolarFloat::new(0.4);
            presets[1].offsets[3].x = BipolarFloat::new(0.99);
            positioner.set_active(1);
            positioner.set_selected_fixture(3);
        }

        // Repatch with two additional fixtures (grow 4 → 6). Preserve the
        // group's GroupId from cfg so the repatch is rename-preserving.
        let stable_id = cfg[0].id;
        let mut grown_cfg = parse(
            "
- fixture: IWashLed
  patches:
    - addr: 1
    - addr: 13
    - addr: 25
    - addr: 37
    - addr: 49
    - addr: 61
",
        )?;
        grown_cfg[0].id = stable_id;
        patch.repatch(grown_cfg.clone().into())?;
        let mut cfg = grown_cfg;

        // Existing offsets survived, new fixtures landed at zero, every
        // slot reconciled — not just slot 0/1.
        {
            let group = patch.iter().next().unwrap();
            let p = group.positioner().expect("positioner survived repatch");

            assert_eq!(p.active(), 1, "active slot preserved");
            assert_eq!(
                p.selected_fixture(),
                3,
                "selected fixture preserved (still in range)",
            );

            let presets = p.presets();
            assert_eq!(presets[0].offsets.len(), 6);
            assert_eq!(presets[0].offsets[0].x.val(), 0.1);
            assert_eq!(presets[0].offsets[1].y.val(), -0.2);
            assert_eq!(presets[0].offsets[2].x.val(), 0.3);
            assert_eq!(presets[0].offsets[3].y.val(), 0.4);
            assert_eq!(presets[0].offsets[4].x.val(), 0.0);
            assert_eq!(presets[0].offsets[4].y.val(), 0.0);
            assert_eq!(presets[0].offsets[5].x.val(), 0.0);

            assert_eq!(presets[1].offsets.len(), 6);
            assert_eq!(presets[1].offsets[3].x.val(), 0.99);
            assert_eq!(presets[1].offsets[4].x.val(), 0.0);

            // Untouched slot is still all zeros at the new length.
            assert_eq!(presets[5].offsets.len(), 6);
            for off in &presets[5].offsets {
                assert_eq!(off.x.val(), 0.0);
                assert_eq!(off.y.val(), 0.0);
            }
        }

        // Repatch shrinking to 3 fixtures — tail entries drop;
        // selected_fixture (which was 3, now out of range) clamps to the
        // new max (2).
        cfg[0].patches.truncate(3);
        patch.repatch(cfg.clone().into())?;

        {
            let group = patch.iter().next().unwrap();
            let p = group.positioner().expect("positioner survived shrink");
            let presets = p.presets();
            assert_eq!(presets[0].offsets.len(), 3);
            assert_eq!(presets[0].offsets[0].x.val(), 0.1);
            assert_eq!(presets[0].offsets[1].y.val(), -0.2);
            assert_eq!(presets[0].offsets[2].x.val(), 0.3);
            // selected_fixture clamped from 3 to 2 (the new last index).
            assert_eq!(p.selected_fixture(), 2);
            // active slot unchanged.
            assert_eq!(p.active(), 1);
        }

        Ok(())
    }

    /// `Patch::configs` returns the configs the patch was built from, and is
    /// updated in lockstep with `repatch`.
    #[test]
    fn patch_retains_configs_across_repatch() -> Result<()> {
        let initial = parse(
            "
- fixture: Dimmer
  patches:
    - addr: 1
",
        )?;
        let mut patch = Patch::patch_all(initial.clone().into())?;
        let initial_ids: Vec<GroupId> = patch.configs().iter().map(|c| c.id).collect();
        let expected_initial_ids: Vec<GroupId> = initial.iter().map(|c| c.id).collect();
        assert_eq!(initial_ids, expected_initial_ids);
        assert_eq!(patch.configs().len(), initial.len());

        let updated = parse(
            "
- fixture: Color
  patches:
    - addr: 1
- fixture: Dimmer
  group: Spare
  patches:
    - addr: 10
",
        )?;
        patch.repatch(updated.clone().into())?;
        let updated_ids: Vec<GroupId> = patch.configs().iter().map(|c| c.id).collect();
        let expected_updated_ids: Vec<GroupId> = updated.iter().map(|c| c.id).collect();
        assert_eq!(updated_ids, expected_updated_ids);
        assert_eq!(patch.configs().len(), updated.len());
        Ok(())
    }

    /// Test that we can patch an instance of WLED.
    #[test]
    fn test_wled() {
        Patch::patch_all(
            parse(
                "
- fixture: Wled
  url: http://foo.bar.baz
  preset_count: 1
  patches:
    -
",
            )
            .unwrap()
            .into(),
        )
        .unwrap();
    }

    /// Test repatching behavior.
    #[test]
    fn test_repatch() -> Result<()> {
        let mut cfg = parse(
            "
- fixture: Color
  control_color_space: Hsluv
  patches:
    - addr: 1
    - addr:
        start: 4
        count: 2
      kind: Rgb
- fixture: Dimmer
  group: TestGroup
  patches:
    - addr: 1
      universe: 1
      mirror: true
    - addr: 12
        ",
        )?;
        let mut patch = Patch::patch_all(cfg.clone().into())?;

        let initial = render(&patch);
        // Should be all zeros, since everything is down.
        assert!(initial.iter().flatten().all(|&v| v == 0));

        // Twiddle some controls and render fixture state.
        for f in patch.iter_mut() {
            twiddle(f);
        }
        let twiddled = render(&patch);

        for (pre, post) in twiddled.iter().zip_eq(&initial) {
            assert_ne!(pre, post);
        }

        // Repatching with the same config should result in so fixture models
        // being updated.
        patch.repatch(cfg.clone().into())?;

        assert_eq!(render(&patch), twiddled);

        // Renaming a group keeps its stable id, so repatching preserves its
        // fixture model state — the operator-typed name changed but the
        // controller knows it's the same group.
        cfg[0].group = Some(GroupName("NewColor".to_string()));

        patch.repatch(cfg.clone().into())?;
        let new_bufs = render(&patch);
        assert_eq!(2, new_bufs.len());
        assert_eq!(
            new_bufs[0], twiddled[0],
            "renaming a group should preserve its state via stable GroupId"
        );
        assert_eq!(new_bufs[1], twiddled[1]);

        // Repatching with different patches should not force a new model.
        cfg[0].patches.truncate(1);
        patch.repatch(cfg.into())?;

        let new_bufs = render(&patch);
        // Should have different output since we have a different number of patches.
        assert_ne!(new_bufs[0], initial[0]);
        assert_ne!(new_bufs[0], twiddled[0]);
        Ok(())
    }

    /// Twiddle some channel-level knobs to move away from initial state.
    fn twiddle(f: &mut FixtureGroup) {
        let _ = f.control_from_channel(
            &crate::channel::ChannelControlMessage::Knob {
                index: 0,
                value: crate::channel::KnobValue::Unipolar(UnipolarFloat::new(0.5)),
            },
            no_op_emitter(),
        );
        let _ = f.control_from_channel(
            &crate::channel::ChannelControlMessage::Level(UnipolarFloat::new(0.75)),
            no_op_emitter(),
        );
    }

    /// Render each group in the patch into a separate buffer.
    fn render(patch: &Patch) -> Vec<DmxBuffer> {
        patch
            .iter()
            .map(|f| {
                let mut dmx = vec![crate::dmx::DmxUniverse::offline()];
                f.render(&Default::default(), &mut dmx, &Default::default());
                dmx[0].buffer
            })
            .collect()
    }

    /// Fixture types that intentionally have no TouchOSC template.
    /// These are either controlled entirely via the fader wing, are utility
    /// types, or need custom templates that haven't been built yet.
    const FIXTURES_WITHOUT_TEMPLATES: &[&str] = &[
        "Comet",
        "EmptyChannel",
        "Leko",
        "RugDoctor",
        "SwarmolonDerby",
        "SwarmolonLasers",
        "SwarmolonStrobe",
        "Venus",
    ];

    #[test]
    fn all_fixture_types_have_touchosc_template() {
        let mut missing = Vec::new();
        let mut empty = Vec::new();
        for patcher in PATCHERS {
            let name = patcher.name.0;
            if FIXTURES_WITHOUT_TEMPLATES.contains(&name) {
                // Explicitly excluded — verify it really has no template.
                assert!(
                    crate::touchosc::load_group_template(name).is_none(),
                    "fixture '{name}' is in FIXTURES_WITHOUT_TEMPLATES but has a template — remove it from the exclusion list"
                );
                continue;
            }
            match crate::touchosc::load_group_template(name) {
                None => missing.push(name),
                Some(Err(e)) => panic!("template for '{name}' failed to parse: {e}"),
                Some(Ok(layout)) => {
                    let page = &layout.tabpages[0];
                    let interactive = page.controls.iter().filter(|c| !c.is_label()).count();
                    if interactive == 0 {
                        empty.push(name);
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "fixture types missing TouchOSC group templates: {missing:?}"
        );
        assert!(
            empty.is_empty(),
            "fixture types with empty TouchOSC templates (no interactive controls): {empty:?}"
        );
    }

    /// Expand an OscControlDescription into the set of address suffixes
    /// that would appear in a template (relative to the group prefix).
    fn expand_control_addresses(control: &OscControlDescription) -> Vec<String> {
        match &control.control_type {
            OscControlType::LabeledSelect { labels } => labels
                .iter()
                .map(|l| format!("{}/{l}", control.name))
                .collect(),
            _ => vec![control.name.clone()],
        }
    }

    /// Extract the control address suffix from a full OSC address.
    /// e.g. "/TriPhase/Dimmer" with group "TriPhase" -> "Dimmer"
    /// e.g. "/Astroscan/Color/Open" with group "Astroscan" -> "Color/Open"
    fn strip_group_prefix<'a>(addr: &'a str, group: &str) -> Option<&'a str> {
        let prefix = format!("/{group}/");
        addr.strip_prefix(&prefix)
    }

    #[test]
    #[ignore]
    fn template_controls_match_fixture_api() {
        use std::collections::BTreeSet;

        let mut mismatches = Vec::new();

        for patcher in PATCHERS {
            let name = patcher.name.0;
            if FIXTURES_WITHOUT_TEMPLATES.contains(&name) {
                continue;
            }

            // Create a fixture group to get its control descriptions.
            let key = GroupName(format!("test_{}", name));
            let id = GroupId::new();
            let group = match (patcher.create_group)(id, key.clone(), Default::default()) {
                Ok(g) => g,
                Err(_) => {
                    let menu = (patcher.group_options)();
                    if menu.is_empty() {
                        continue;
                    }
                    let options = Options::from_entries(
                        menu.iter().map(|(k, opt)| (k.clone(), opt.example_value())),
                    );
                    match (patcher.create_group)(id, key.clone(), options) {
                        Ok(g) => g,
                        Err(_) => continue,
                    }
                }
            };

            let controls = group.describe_controls();
            let api_addrs: BTreeSet<String> =
                controls.iter().flat_map(expand_control_addresses).collect();

            // Load the template and extract OSC addresses.
            let layout = match crate::touchosc::load_group_template(name) {
                Some(Ok(l)) => l,
                _ => continue,
            };
            let page = &layout.tabpages[0];
            // Verify all controls have the correct group prefix, and collect suffixes.
            let mut template_addrs: BTreeSet<String> = BTreeSet::new();
            let mut wrong_prefix = Vec::new();
            for ctrl in &page.controls {
                if let Some(addr) = ctrl.osc_address() {
                    if let Some(suffix) = strip_group_prefix(addr, name) {
                        template_addrs.insert(suffix.to_string());
                    } else {
                        wrong_prefix.push(addr.to_string());
                    }
                }
            }
            if !wrong_prefix.is_empty() {
                mismatches.push(format!(
                    "{name}:\n  controls with wrong group prefix (expected /{name}/...): {wrong_prefix:?}"
                ));
                continue;
            }

            let in_api_not_template: BTreeSet<_> = api_addrs.difference(&template_addrs).collect();
            let in_template_not_api: BTreeSet<_> = template_addrs.difference(&api_addrs).collect();

            if !in_api_not_template.is_empty() || !in_template_not_api.is_empty() {
                let mut msg = format!("{name}:");
                if !in_api_not_template.is_empty() {
                    msg += &format!("\n  in API but not template: {:?}", in_api_not_template);
                }
                if !in_template_not_api.is_empty() {
                    msg += &format!("\n  in template but not API: {:?}", in_template_not_api);
                }
                mismatches.push(msg);
            }
        }

        if !mismatches.is_empty() {
            panic!(
                "Template/API control mismatches:\n{}",
                mismatches.join("\n")
            );
        }
    }
}
