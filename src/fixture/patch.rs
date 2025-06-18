//! Types and traits related to patching fixtures.
use anyhow::{anyhow, ensure, Result};
use itertools::Itertools;
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
    /// Collection of closures that patch fixture types.
    ///
    /// FIXME: since we have global registration now, we might be able to avoid
    /// the need for closures and collect the actual patcher functions
    /// themselves.
    patchers: Vec<Patcher>,
    /// The fixture groups we've patched.
    ///
    /// TODO: by using a HashMap for fast key lookup, we do not have a defined
    /// patch ordering. This implies that iterating over the patch will produce
    /// a stable but random order. This probably isn't important.
    fixtures: HashMap<FixtureGroupKey, FixtureGroup>,
    /// Lookup from static fixture type strings to FixtureType instances.
    fixture_type_lookup: HashMap<&'static str, FixtureType>,
    /// Which DMX addrs already have a fixture patched in them.
    used_addrs: UsedAddrs,
    /// The channels that fixture groups are assigned to.
    channels: Vec<FixtureGroupKey>,
}

/// Distributed registry for things that we can patch.
///
/// Fixtures use the register macro to add themselves to this collection.
/// The derive macros for the patch traits handle this.
#[distributed_slice]
pub static PATCHERS: [fn() -> Patcher];

/// Register a patcher-generating function with the patch.
#[macro_export]
macro_rules! register {
    ($fixture:ty) => {
        use linkme::distributed_slice;
        use $crate::fixture::patch::{Patcher, PATCHERS};

        #[distributed_slice(PATCHERS)]
        static PATCHER: fn() -> Patcher = <$fixture>::patcher;
    };
}

impl Patch {
    /// Initialize a new fixture patch.
    ///
    /// The patchers are initialized from the global registry.
    pub fn new() -> Self {
        let patchers: Vec<_> = PATCHERS.iter().map(|p| p()).collect();
        assert!(!patchers.is_empty());
        Self {
            patchers,
            fixtures: Default::default(),
            fixture_type_lookup: Default::default(),
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
        let candidate = self.get_candidate(&cfg.name, &cfg.options)?;
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
        let mut candidates = self
            .patchers
            .iter()
            .flat_map(|p| p(name, options))
            .collect::<Result<Vec<_>>>()?;
        let candidate = match candidates.len() {
            0 => bail!("unable to patch {name}"),
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
        let candidate = self.get_candidate(&cfg.name, &cfg.options)?;
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
                cfg.name,
                addr,
                cfg.group.as_deref().unwrap_or("none")
            );
        } else {
            ensure!(
                candidate.channel_count == 0,
                "No DMX address provided for DMX-controlled fixture {}",
                candidate.fixture_type
            );
            info!(
                "Controlling {} (non-DMX fixture) (group: {}).",
                cfg.name,
                cfg.group.as_deref().unwrap_or("none")
            );
        }

        let key = FixtureGroupKey {
            fixture: candidate.fixture_type,
            group: cfg.group,
        };
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

        self.fixture_type_lookup.insert(key.fixture.0, key.fixture);
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
                        cfg.name,
                        dmx_addr,
                        addr + 1,
                        cfg.universe,
                        existing_fixture.name,
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

    /// Look up the static version of a fixture type registered with the patch.
    pub fn lookup_fixture_type(&self, t: &str) -> Option<FixtureType> {
        self.fixture_type_lookup.get(t).copied()
    }

    /// Get the fixture/channel patched with this key.
    pub fn get(&self, key: &FixtureGroupKey) -> Result<&FixtureGroup> {
        self.fixtures
            .get(key)
            .ok_or_else(|| anyhow!("fixture {key:?} not found in patch"))
    }

    /// Get the fixture/channel patched with this key, mutably.
    pub fn get_mut(&mut self, key: &FixtureGroupKey) -> Result<&mut FixtureGroup> {
        self.fixtures
            .get_mut(key)
            .ok_or_else(|| anyhow!("fixture {key:?} not found in patch"))
    }

    /// Iterate over all patched fixtures.
    pub fn iter(&self) -> impl Iterator<Item = &FixtureGroup> {
        self.fixtures.values()
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

pub type Patcher = Box<dyn Fn(&str, &Options) -> Option<Result<PatchCandidate>> + Sync>;

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchFixture: NonAnimatedFixture + Default + 'static {
    const NAME: FixtureType;

    /// Return a closure that will try to patch a fixture if it has the appropriate name.
    fn patcher() -> Patcher {
        Box::new(|name: &str, options: &Options| {
            if *name != *Self::NAME {
                return None;
            }
            match Self::new(options) {
                Ok((fixture, render_mode)) => Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(render_mode),
                    render_mode,
                    fixture: Box::new(fixture),
                })),
                Err(e) => Some(Err(e)),
            }
        })
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    fn new(_options: &Options) -> Result<(Self, Option<RenderMode>)> {
        Ok((Self::default(), None))
    }
}

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchAnimatedFixture: AnimatedFixture + Default + 'static {
    const NAME: FixtureType;

    /// Return a closure that will try to patch a fixture if it has the appropriate name.
    fn patcher() -> Patcher {
        Box::new(|name, options| {
            if *name != *Self::NAME {
                return None;
            }
            match Self::new(options) {
                Ok((fixture, render_mode)) => Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(render_mode),
                    render_mode,
                    fixture: Box::new(FixtureWithAnimations {
                        fixture,
                        animations: Default::default(),
                    }),
                })),
                Err(e) => Some(Err(e)),
            }
        })
    }

    /// The number of contiguous DMX channels used by the fixture.
    ///
    /// A render mode is provided for fixtures that may have different channel
    /// counts for different individual specific fixtures.
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    fn new(_options: &Options) -> Result<(Self, Option<RenderMode>)> {
        Ok((Self::default(), None))
    }
}
