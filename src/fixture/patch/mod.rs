//! Types and traits related to patching fixtures.
use anyhow::{anyhow, ensure, Context, Result};
use itertools::Itertools;
use ordermap::{OrderMap, OrderSet};
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Write};

use anyhow::bail;
use log::info;

use super::fixture::{
    AnimatedFixture, FixtureType, FixtureWithAnimations, NonAnimatedFixture, RenderMode,
};
use super::group::FixtureGroup;
use crate::config::{FixtureGroupConfig, FixtureGroupKey, Options};
use crate::dmx::UniverseIdx;
use crate::fixture::group::GroupFixtureConfig;
use linkme::distributed_slice;

mod option;
mod patcher;

pub use patcher::{
    CreateAnimatedGroup, CreateNonAnimatedGroup, PatchConfig, PatchFixture, Patcher, PATCHERS,
};

pub use option::{enum_patch_option, AsPatchOption, NoOptions, OptionsMenu, PatchOption};

/// Mapping between a universe/address pair and the type of fixture already
/// addressed over that pair, as well as the starting address.
#[derive(Default, Clone)]
struct UsedAddrs(HashMap<(UniverseIdx, usize), (FixtureType, usize)>);

impl UsedAddrs {
    /// Attempt to allocate requested addresses for the provided fixture type.
    ///
    /// The addresses will only be allocated if there are no conflicts.
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
            debug_assert!(existing.is_none());
        }

        Ok(())
    }
}

/// Factory for fixture instances.
///
/// Creates fixture instances based on configurations.
/// Maintains a mapping of which DMX addresses are in use by which fixture, to
/// prevent addressing collisions.
pub struct Patch {
    /// Map of registered patchers.
    patchers: HashMap<String, Patcher>,

    /// The fixture groups we've patched.
    fixtures: OrderMap<FixtureGroupKey, FixtureGroup>,
    /// Which DMX addrs already have a fixture patched in them.
    used_addrs: UsedAddrs,
    /// The channels that fixture groups are assigned to.
    channels: OrderSet<FixtureGroupKey>,
    /// Initialize color organs for these fixture groups.
    color_organs: OrderSet<FixtureGroupKey>,
}

impl Patch {
    /// Return the full menu of fixtures we can patch, sorted by name.
    pub fn menu() -> Vec<Patcher> {
        PATCHERS.iter().cloned().sorted_by_key(|p| p.name).collect()
    }

    /// Initialize a new fixture patch.
    pub fn new() -> Self {
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
            fixtures: Default::default(),
            used_addrs: Default::default(),
            channels: Default::default(),
            color_organs: Default::default(),
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
    pub fn patch_all(groups: Vec<FixtureGroupConfig>) -> Result<Self> {
        let mut patch = Self::new();
        for group in groups {
            patch.patch(&group).with_context(|| {
                format!(
                    "patching {}{}",
                    group.fixture,
                    group.group.map(|g| format!("({g})")).unwrap_or_default()
                )
            })?;
        }
        Ok(patch)
    }

    /// Re-intialize a patch from new configs.
    ///
    /// This allows retaining all existing state for any groups that haven't
    /// materially changed.
    ///
    /// If any patch error occurs, we must ensure that the original patch remains
    /// unchanged. This is a bit tricky, since some fixture types may perform
    /// complex initialization of external resources like sockets and so we need
    /// to make sure that we've released resources before patching anything new.
    fn repatch(&mut self, groups: Vec<FixtureGroupConfig>) -> Result<()> {
        // Step 1: divide up groups into things we have already vs. things
        // that we need to add or replace.
        let (keep_existing, make_new): (Vec<_>, Vec<_>) = groups.into_iter().partition(|g| {
            let Some(existing) = self.fixtures.get(g.key()) else {
                // No existing patch matches the provided key.
                return false;
            };
            if existing.fixture_type().as_ref() != g.fixture.as_str() {
                return false;
            }
            // Same key, same fixture type; if the options match, keep it.
            existing.options() == &g.options
        });
        todo!();
        Ok(())
    }

    /// Patch a fixture group config.
    ///
    ///
    fn patch(&mut self, cfg: &FixtureGroupConfig) -> Result<()> {
        let patcher = self.patcher(&cfg.fixture)?;

        if let Some(group_key) = &cfg.group {
            // If there's a patcher that matches this group, fail.
            ensure!(
                self.patcher(group_key).is_err(),
                "the group key '{group_key}' cannot be used because it is also a fixture name"
            );
        }

        let group_key = FixtureGroupKey(cfg.key().to_string());

        ensure!(
            !self.fixtures.contains_key(&group_key),
            "duplicate group key '{group_key}'"
        );

        let mut group = (patcher.create_group)(group_key.clone(), cfg.options.clone())?;

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

        self.fixtures.insert(group_key.clone(), group);

        if cfg.channel {
            self.channels.insert(group_key.clone());
        }
        if cfg.color_organ {
            self.color_organs.insert(group_key.clone());
        }
        Ok(())
    }

    /// Iterate over the fixture group keys assigned to each channel.
    pub fn channels(&self) -> impl Iterator<Item = &FixtureGroupKey> + '_ {
        self.channels.iter()
    }

    /// Initialize color organs for all fixtures that should have them.
    ///
    /// This should be called after all fixtures are patched.
    /// TODO: update the color organ codebase to handle a change in fixture count.
    pub fn initialize_color_organs(&mut self) {
        for key in &self.color_organs {
            self.fixtures[key].use_color_organ();
        }
    }

    /// Dynamically get the universe count.
    pub fn universe_count(&self) -> usize {
        let mut universes = HashSet::new();
        for group in self.fixtures.values() {
            for element in group.fixture_configs() {
                universes.insert(element.universe);
            }
        }
        universes.len()
    }
    /// Get the fixture/channel patched with this key.
    pub fn get(&self, key: &str) -> Result<&FixtureGroup> {
        self.fixtures
            .get(key)
            .ok_or_else(|| anyhow!("fixture {key} not found in patch"))
    }

    /// Get the fixture/channel patched with this key, mutably.
    pub fn get_mut(&mut self, key: &str) -> Result<&mut FixtureGroup> {
        self.fixtures
            .get_mut(key)
            .ok_or_else(|| anyhow!("fixture {key} not found in patch"))
    }

    /// Iterate over all patched fixtures.
    pub fn iter(&self) -> impl Iterator<Item = &FixtureGroup> {
        self.fixtures.values()
    }

    /// Iterate over all patched fixtures along with their keys.
    pub fn iter_with_keys(&self) -> impl Iterator<Item = (&FixtureGroupKey, &FixtureGroup)> {
        self.fixtures.iter()
    }

    /// Iterate over all patched fixtures, mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut FixtureGroup> {
        self.fixtures.values_mut()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        config::FixtureGroupConfig,
        fixture::{color::Model as ColorModel, fixture::EnumRenderModel},
    };
    use anyhow::Result;

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
        let p = Patch::patch_all(cfg)?;
        assert_eq!(
            "Color",
            p.channels.iter().exactly_one().unwrap().to_string()
        );
        assert_eq!(2, p.fixtures.len());
        let color_configs = p.get("Color")?.fixture_configs();
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
        let dimmer_configs = p.get("TestGroup")?.fixture_configs();
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
                .expect("invalid patch format"),
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
    fn test_dupe_group_key() {
        // Can't specify the same fixture twice with no group.
        assert_fail_patch(
            "
- fixture: Dimmer
  patches:
    - addr: 1
- fixture: Dimmer
  patches:
    - addr: 2",
            "duplicate group key 'Dimmer'",
        );
        // Can't use the same group key twice.
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
            "duplicate group key 'Foo'",
        );
    }

    #[test]
    fn test_no_aliasing_fixture() {
        // Can't use a group key that collides with a fixture name.
        assert_fail_patch(
            "
- fixture: Dimmer
  group: Color
  patches:
    - addr: 1",
            "the group key 'Color' cannot be used because it is also a fixture name",
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
            .unwrap(),
        )
        .unwrap();
    }
}
