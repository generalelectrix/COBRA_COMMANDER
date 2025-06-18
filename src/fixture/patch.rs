//! Types and traits related to patching fixtures.
use anyhow::{anyhow, ensure, Context, Result};
use itertools::Itertools;
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};

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
    ///
    /// TODO: by using a HashMap for fast key lookup, we do not have a defined
    /// patch ordering. This implies that iterating over the patch will produce
    /// a stable but random order. This probably isn't important.
    fixtures: HashMap<FixtureGroupKey, FixtureGroup>,
    /// Which DMX addrs already have a fixture patched in them.
    used_addrs: UsedAddrs,
    /// The channels that fixture groups are assigned to.
    channels: Vec<FixtureGroupKey>,
}

/// Distributed registry for things that we can patch.
///
/// The derive macros for the patch traits handle this.
/// Use the register_patcher macro for fixtures that cannot derive patch.
#[distributed_slice]
pub static PATCHERS: [Patcher];

impl Patch {
    /// Initialize a new fixture patch.
    pub fn new() -> Self {
        assert!(!PATCHERS.is_empty());
        Self {
            fixtures: Default::default(),
            used_addrs: Default::default(),
            channels: Default::default(),
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
            self.patch_one(fixture_cfg)?;
        }
        Ok(())
    }

    /// Iterate over the fixture group keys assigned to each channel.
    pub fn channels(&self) -> impl Iterator<Item = FixtureGroupKey> + '_ {
        self.channels.iter().cloned()
    }

    fn get_candidate(&self, name: &str, options: &Options) -> Result<PatchCandidate> {
        let mut candidates = PATCHERS
            .iter()
            .flat_map(|p| p(name, options))
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
    fn patch_one(&mut self, cfg: FixtureConfig) -> anyhow::Result<()> {
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
        // Add channel mapping index if provided.  Ensure this is an animatable fixture.
        if cfg.channel {
            ensure!(
                candidate.fixture.is_animated(),
                "cannot assign non-animatable fixture {} to a channel",
                candidate.fixture_type
            );
        }
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
            return Ok(());
        }
        // No existing group; create a new one.
        if cfg.channel {
            self.channels.push(key.clone());
        }

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

        self.fixtures.insert(key, group);

        Ok(())
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
    pub fn get<Q>(&self, key: &Q) -> Result<&FixtureGroup>
    where
        Q: std::hash::Hash + Eq + ?Sized + std::fmt::Display,
        FixtureGroupKey: Borrow<Q>,
    {
        self.fixtures
            .get(key)
            .ok_or_else(|| anyhow!("fixture {key} not found in patch"))
    }

    /// Get the fixture/channel patched with this key, mutably.
    pub fn get_mut<Q>(&mut self, key: &Q) -> Result<&mut FixtureGroup>
    where
        Q: std::hash::Hash + Eq + ?Sized + std::fmt::Display,
        FixtureGroupKey: Borrow<Q>,
    {
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

pub type Patcher = fn(&str, &Options) -> Option<Result<PatchCandidate>>;

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchFixture: NonAnimatedFixture + Default + 'static {
    const NAME: FixtureType;

    /// Return a PatchCandidate for this fixture if it has the appropriate name.
    fn patch(name: &str, options: &Options) -> Option<Result<PatchCandidate>> {
        if *name != *Self::NAME {
            return None;
        }
        let mut options = options.clone();
        match Self::new(&mut options) {
            Ok((fixture, render_mode)) => {
                // Ensure the fixture processed all of the options.
                if !options.is_empty() {
                    return Some(Err(anyhow!(
                        "unhandled options: {}",
                        options.keys().join(", ")
                    )));
                }
                Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(render_mode),
                    render_mode,
                    fixture: Box::new(fixture),
                }))
            }
            Err(e) => Some(Err(e)),
        }
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    ///
    /// Fixtures should remove all recognized items from Options.
    /// Any unhandled options remaining will result in a patch error.
    fn new(_options: &mut Options) -> Result<(Self, Option<RenderMode>)> {
        Ok((Self::default(), None))
    }
}

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchAnimatedFixture: AnimatedFixture + Default + 'static {
    const NAME: FixtureType;

    /// Return a PatchCandidate for this fixture if it has the appropriate name.
    fn patch(name: &str, options: &Options) -> Option<Result<PatchCandidate>> {
        if *name != *Self::NAME {
            return None;
        }
        let mut options = options.clone();
        match Self::new(&mut options) {
            Ok((fixture, render_mode)) => {
                // Ensure the fixture processed all of the options.
                if !options.is_empty() {
                    return Some(Err(anyhow!(
                        "unknown options: {}",
                        options.keys().join(", ")
                    )));
                }
                Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(render_mode),
                    render_mode,
                    fixture: Box::new(FixtureWithAnimations {
                        fixture,
                        animations: Default::default(),
                    }),
                }))
            }
            Err(e) => Some(Err(e)),
        }
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    ///
    /// Fixtures should remove all recognized items from Options.
    /// Any unhandled options remaining will result in a patch error.
    fn new(_options: &mut Options) -> Result<(Self, Option<RenderMode>)> {
        Ok((Self::default(), None))
    }
}
