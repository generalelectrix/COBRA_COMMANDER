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
    AnimatedFixture, Fixture, FixtureType, FixtureWithAnimations, NonAnimatedFixture, RenderMode,
};
use super::group::{FixtureGroup, FixtureGroupKey};
use crate::config::{FixtureConfig, FixtureGroupConfig, Options};
use crate::dmx::UniverseIdx;
use crate::fixture::group::GroupFixtureConfig;
use linkme::distributed_slice;

type UsedAddrs = HashMap<(UniverseIdx, usize), FixtureConfig>;

/// Factory for fixture instances.
///
/// Creates fixture instances based on configurations.
/// Maintains a mapping of which DMX addresses are in use by which fixture, to
/// prevent addressing collisions.
pub struct Patch {
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
        Self {
            fixtures: Default::default(),
            used_addrs: Default::default(),
            channels: Default::default(),
            color_organs: Default::default(),
        }
    }

    /// Initialize a patch from a collection of fixtures.
    pub fn patch_all(fixtures: impl IntoIterator<Item = FixtureGroupConfig>) -> Result<Self> {
        let mut patch = Self::new();
        for fixture in fixtures {
            patch.patch(fixture)?;
        }
        Ok(patch)
    }

    /// Patch a fixture group config - either a single address or a range.
    pub fn patch(&mut self, cfg: FixtureGroupConfig) -> anyhow::Result<()> {
        let candidate = self.get_candidate(&cfg.fixture, &cfg.options)?;

        for fixture_cfg in cfg.fixture_configs(candidate.channel_count) {
            let key = self.patch_one(fixture_cfg)?;
            if cfg.channel {
                self.channels.insert(key.clone());
            }
            if cfg.color_organ {
                self.color_organs.insert(key.clone());
            }
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

    fn get_candidate(&self, name: &str, options: &Options) -> Result<PatchCandidate> {
        let mut candidates = PATCHERS
            .iter()
            .filter_map(|p| {
                if *p.name != *name {
                    return None;
                }
                Some((p.func)(options))
            })
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("patching {name}"))?;
        let candidate = match candidates.len() {
            0 => bail!("unrecognized fixture type {name}"),
            1 => candidates.pop().unwrap(),
            _ => bail!(
                "multiple fixture patch candidates: {:?}",
                candidates.iter().map(|c| &c.fixture_type).join(", ")
            ),
        };
        Ok(candidate)
    }

    /// Patch a single fixture config.
    ///
    /// Return the group key.
    fn patch_one(&mut self, cfg: FixtureConfig) -> anyhow::Result<FixtureGroupKey> {
        let candidate = self
            .get_candidate(&cfg.fixture, &cfg.options)
            .with_context(|| {
                if let Some(dmx_addr) = cfg.addr {
                    format!("address {dmx_addr} (universe {})", cfg.universe)
                } else {
                    String::new()
                }
            })?;
        self.used_addrs = self.check_collision(&candidate, &cfg)?;
        if let Some(addr) = cfg.addr {
            info!(
                "Controlling {} at {} (group: {}).",
                cfg.fixture,
                addr,
                cfg.group.as_deref().unwrap_or("none")
            );
        } else {
            ensure!(
                candidate.channel_count == 0,
                "no DMX address provided for DMX-controlled fixture {}",
                candidate.fixture_type
            );
            info!(
                "Controlling {} (non-DMX fixture) (group: {}).",
                cfg.fixture,
                cfg.group.as_deref().unwrap_or("none")
            );
        }

        let key = FixtureGroupKey(
            cfg.group
                .unwrap_or_else(|| candidate.fixture_type.to_string()),
        );
        // Either identify an existing appropriate group or create a new one.
        if let Some(group) = self.fixtures.get_mut(&key) {
            group.patch(GroupFixtureConfig {
                universe: cfg.universe,
                dmx_addr: cfg.addr.map(|a| a.dmx_index()),
                channel_count: candidate.channel_count,
                render_mode: candidate.render_mode,
                mirror: cfg.mirror,
            });
            return Ok(key);
        }
        // No existing group; create a new one.
        let group = FixtureGroup::new(
            candidate.fixture_type,
            key.clone(),
            GroupFixtureConfig {
                universe: cfg.universe,
                dmx_addr: cfg.addr.map(|a| a.dmx_index()),
                channel_count: candidate.channel_count,
                render_mode: candidate.render_mode,
                mirror: cfg.mirror,
            },
            candidate.fixture,
        );

        self.fixtures.insert(key.clone(), group);

        Ok(key)
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
        candidate: &PatchCandidate,
        cfg: &FixtureConfig,
    ) -> Result<UsedAddrs> {
        let mut used_addrs = self.used_addrs.clone();
        let Some(dmx_addr) = cfg.addr else {
            return Ok(used_addrs);
        };
        let dmx_index = dmx_addr.dmx_index();
        for addr in dmx_index..dmx_index + candidate.channel_count {
            match used_addrs.get(&(cfg.universe, addr)) {
                Some(existing_fixture) => {
                    bail!(
                        "{} at {} overlaps at DMX address {} in universe {} with {} at {}.",
                        cfg.fixture,
                        dmx_addr,
                        addr + 1,
                        cfg.universe,
                        existing_fixture.fixture,
                        // Existing fixtures must have an address to have ended up in used_addrs.
                        existing_fixture.addr.unwrap(),
                    );
                }
                None => {
                    used_addrs.insert((cfg.universe, addr), cfg.clone());
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

pub struct PatchCandidate {
    fixture_type: FixtureType,
    channel_count: usize,
    render_mode: Option<RenderMode>,
    fixture: Box<dyn Fixture>,
}

#[derive(Clone)]
pub struct Patcher {
    pub name: FixtureType,
    pub func: fn(&Options) -> Result<PatchCandidate>,
    pub options: fn() -> Vec<(String, PatchOption)>,
}

impl Display for Patcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;

        let opts = (self.options)();
        // If a fixture doesn't take options, we should be able to get a channel count.
        if opts.is_empty() {
            if let Ok(fix) = (self.func)(&Default::default()) {
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

        let opts = (self.options)();
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

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchFixture: NonAnimatedFixture + Sized + 'static {
    const NAME: FixtureType;

    /// Return a PatchCandidate for this fixture if it has the appropriate name.
    fn patch(options: &Options) -> Result<PatchCandidate> {
        let mut options = options.clone();
        let (fixture, render_mode) = Self::new(&mut options)?;
        // Ensure the fixture processed all of the options.
        if !options.is_empty() {
            return Err(anyhow!("unhandled options: {}", options.keys().join(", ")));
        }
        Ok(PatchCandidate {
            fixture_type: Self::NAME,
            channel_count: fixture.channel_count(render_mode),
            render_mode,
            fixture: Box::new(fixture),
        })
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Return the menu of patch options for this fixture type.
    fn options() -> Vec<(String, PatchOption)>;

    /// Create a new instance of the fixture from the provided options.
    ///
    /// Fixtures should remove all recognized items from Options.
    /// Any unhandled options remaining will result in a patch error.
    fn new(options: &mut Options) -> Result<(Self, Option<RenderMode>)>;
}

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchAnimatedFixture: AnimatedFixture + Sized + 'static {
    const NAME: FixtureType;

    /// Return a PatchCandidate for this fixture if it has the appropriate name.
    fn patch(options: &Options) -> Result<PatchCandidate> {
        let mut options = options.clone();
        let (fixture, render_mode) = Self::new(&mut options)?;
        // Ensure the fixture processed all of the options.
        if !options.is_empty() {
            return Err(anyhow!("unknown options: {}", options.keys().join(", ")));
        }
        Ok(PatchCandidate {
            fixture_type: Self::NAME,
            channel_count: fixture.channel_count(render_mode),
            render_mode,
            fixture: Box::new(FixtureWithAnimations {
                fixture,
                animations: Default::default(),
            }),
        })
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Return the menu of patch options for this fixture type.
    fn options() -> Vec<(String, PatchOption)>;

    /// Create a new instance of the fixture from the provided options.
    ///
    /// Fixtures should remove all recognized items from Options.
    /// Any unhandled options remaining will result in a patch error.
    fn new(options: &mut Options) -> Result<(Self, Option<RenderMode>)>;
}

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
