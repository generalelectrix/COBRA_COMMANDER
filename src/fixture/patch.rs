//! Types and traits related to patching fixtures.
use anyhow::{anyhow, ensure, Context, Result};
use itertools::Itertools;
use ordermap::{OrderMap, OrderSet};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Write};
use strum::IntoEnumIterator;

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

/// Mapping between a universe/address pair and the type of fixture already
/// addressed over that pair, as well as the starting address.
type UsedAddrs = HashMap<(UniverseIdx, usize), (FixtureType, usize)>;

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

/// Distributed registry for things that we can patch.
///
/// The derive macros for the patch traits handle this.
/// Use the register_patcher macro for fixtures that cannot derive patch.
#[distributed_slice]
pub static PATCHERS: [Patcher];

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

    /// Initialize a patch from a collection of fixtures.
    pub fn patch_all(fixtures: impl IntoIterator<Item = FixtureGroupConfig>) -> Result<Self> {
        let mut patch = Self::new();
        for fixture in fixtures {
            patch.patch(&fixture).with_context(|| {
                format!(
                    "patching {}{}",
                    fixture.fixture,
                    fixture.group.map(|g| format!("({g})")).unwrap_or_default()
                )
            })?;
        }
        Ok(patch)
    }

    /// Patch a fixture group config.
    fn patch(&mut self, cfg: &FixtureGroupConfig) -> Result<()> {
        let patcher = self.patcher(&cfg.fixture)?;

        let group_key = cfg
            .group
            .clone()
            .unwrap_or_else(|| FixtureGroupKey(cfg.fixture.to_string()));

        let mut group = (patcher.create_group)(group_key.clone(), &cfg.options)?;

        // FIXME: should do some validation around having at least one patch
        // block... but need to decide how to handle patching non-DMX fixtures.

        for block in &cfg.patches {
            let patch_cfg = (patcher.patch)(&block.options)
                .with_context(|| group.qualified_name().to_string())?;
            let (start_addr, count) = block.start_count();
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
                        dmx_addr: None,
                        universe: block.universe,
                        channel_count: patch_cfg.channel_count,
                        mirror: block.mirror,
                        render_mode: patch_cfg.render_mode,
                    });
                }
                Some(mut dmx_addr) => {
                    for _ in 0..count {
                        let fixture_cfg = GroupFixtureConfig {
                            dmx_addr: Some(dmx_addr.dmx_index()),
                            universe: block.universe,
                            channel_count: patch_cfg.channel_count,
                            mirror: block.mirror,
                            render_mode: patch_cfg.render_mode,
                        };

                        self.used_addrs = self.check_collision(patcher.name, &fixture_cfg)?;

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

    /// Check that the patch candidate doesn't conflict with another patched fixture.
    /// Return an updated collection of used addresses if it does not conflict.
    fn check_collision(
        &self,
        fixture_type: FixtureType,
        cfg: &GroupFixtureConfig,
    ) -> Result<UsedAddrs> {
        let mut used_addrs = self.used_addrs.clone();
        let Some(dmx_index) = cfg.dmx_addr else {
            return Ok(used_addrs);
        };
        for index in dmx_index..dmx_index + cfg.channel_count {
            match used_addrs.get(&(cfg.universe, index)) {
                Some((existing_fixture, patched_at)) => {
                    bail!(
                        "{} at {} overlaps at DMX address {} in universe {} with {} at {}.",
                        fixture_type,
                        dmx_index + 1,
                        index + 1,
                        cfg.universe,
                        existing_fixture,
                        patched_at + 1,
                    );
                }
                None => {
                    used_addrs.insert((cfg.universe, index), (fixture_type, dmx_index));
                }
            }
        }
        Ok(used_addrs)
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

pub struct PatchConfig {
    pub channel_count: usize,
    pub render_mode: Option<RenderMode>,
}

#[derive(Clone)]
pub struct Patcher {
    pub name: FixtureType,
    pub patch: fn(&Options) -> Result<PatchConfig>,
    pub patch_options: fn() -> Vec<(String, PatchOption)>,
    pub create_group: fn(FixtureGroupKey, &Options) -> Result<FixtureGroup>,
    pub group_options: fn() -> Vec<(String, PatchOption)>,
}

impl Display for Patcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;

        let opts = (self.patch_options)();
        // If a fixture doesn't take options, we should be able to get a channel count.
        if opts.is_empty() {
            if let Ok(fix) = (self.patch)(&Default::default()) {
                if fix.channel_count > 0 {
                    write!(
                        f,
                        " ({} channel{})",
                        fix.channel_count,
                        if fix.channel_count > 1 { "s" } else { "" }
                    )?;
                }
            }
        }

        let opts = (self.patch_options)();
        if opts.is_empty() {
            return Ok(());
        }
        f.write_char('\n')?;
        writeln!(f, "  options:")?;
        for (key, opt) in opts {
            writeln!(f, "    {key}: {opt}")?;
        }
        Ok(())
    }
}

pub trait PatchFixture: Sized + 'static {
    const NAME: FixtureType;

    /// Return a PatchCandidate for this fixture.
    fn patch(options: &Options) -> Result<PatchConfig> {
        let mut options = options.clone();
        let cfg = Self::patch_config(&mut options)?;
        // Ensure the fixture processed all of the options.
        if !options.is_empty() {
            return Err(anyhow!("unhandled options: {}", options.keys().join(", ")));
        }
        Ok(cfg)
    }

    /// Return the menu of patch options for this fixture type.
    fn patch_options() -> Vec<(String, PatchOption)>;

    /// Create a patch configuration for this fixture from the provided options.
    fn patch_config(options: &mut Options) -> Result<PatchConfig>;

    /// Create a new instance of the fixture from the provided options.
    ///
    /// Fixtures should remove all recognized items from Options.
    /// Any unhandled options remaining will result in a patch error.
    fn new(options: &mut Options) -> Result<Self>;

    /// Return the menu of group options for this fixture type.
    fn group_options() -> Vec<(String, PatchOption)>;
}

/// Create a fixture group for a non-animated fixture.
pub trait CreateNonAnimatedGroup: PatchFixture + NonAnimatedFixture + Sized + 'static {
    /// Create an empty fixture group for this type of fixture.
    fn create_group(key: FixtureGroupKey, options: &Options) -> Result<FixtureGroup> {
        let mut options = options.clone();
        let fixture = Self::new(&mut options)?;
        if !options.is_empty() {
            return Err(anyhow!("unhandled options: {}", options.keys().join(", ")));
        }
        Ok(FixtureGroup::empty(Self::NAME, key, Box::new(fixture)))
    }
}

impl<T> CreateNonAnimatedGroup for T where T: PatchFixture + NonAnimatedFixture + Sized + 'static {}

/// Create a fixture group for an animated fixture.
pub trait CreateAnimatedGroup: PatchFixture + AnimatedFixture + Sized + 'static {
    /// Create an empty fixture group for this type of fixture.
    fn create_group(key: FixtureGroupKey, options: &Options) -> Result<FixtureGroup> {
        let mut options = options.clone();
        let fixture = Self::new(&mut options)?;
        if !options.is_empty() {
            return Err(anyhow!("unhandled options: {}", options.keys().join(", ")));
        }
        Ok(FixtureGroup::empty(
            Self::NAME,
            key,
            Box::new(FixtureWithAnimations {
                fixture,
                animations: Default::default(),
            }),
        ))
    }
}

impl<T> CreateAnimatedGroup for T where T: PatchFixture + AnimatedFixture + Sized + 'static {}

/// The kinds of patch options that fixtures can specify.
pub enum PatchOption {
    /// Select a specific option from a menu.
    Select(Vec<String>),

    /// A network address.
    SocketAddr,
}

impl Display for PatchOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Select(opts) => f.write_str(&opts.join(", ")),
            Self::SocketAddr => f.write_str("<socket address>"),
        }
    }
}

/// Things that can be converted into patch options.
pub trait AsPatchOption {
    fn patch_option() -> PatchOption;
}

impl<T> AsPatchOption for T
where
    T: IntoEnumIterator + Display,
{
    fn patch_option() -> PatchOption {
        PatchOption::Select(Self::iter().map(|x| x.to_string()).collect())
    }
}
