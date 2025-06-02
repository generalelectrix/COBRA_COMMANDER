use anyhow::{anyhow, ensure, Result};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};

use anyhow::bail;
use lazy_static::lazy_static;
use log::info;

use super::fixture::{
    AnimatedFixture, Fixture, FixtureType, FixtureWithAnimations, NonAnimatedFixture,
};
use super::group::{FixtureGroup, FixtureGroupKey};
use super::profile::aquarius::Aquarius;
use super::profile::astroscan::Astroscan;
use super::profile::color::Color;
use super::profile::colordynamic::Colordynamic;
use super::profile::comet::Comet;
use super::profile::dimmer::Dimmer;
use super::profile::faderboard::Faderboard;
use super::profile::freedom_fries::FreedomFries;
use super::profile::h2o::H2O;
use super::profile::hypnotic::Hypnotic;
use super::profile::lumasphere::Lumasphere;
use super::profile::radiance::Radiance;
use super::profile::rotosphere_q3::RotosphereQ3;
use super::profile::rush_wizard::RushWizard;
use super::profile::solar_system::SolarSystem;
use super::profile::starlight::Starlight;
use super::profile::uv_led_brick::UvLedBrick;
use super::profile::venus::Venus;
use super::profile::wizard_extreme::WizardExtreme;
use crate::channel::Channels;
use crate::config::{FixtureConfig, FixtureGroupConfig, Options};
use crate::dmx::UniverseIdx;
use crate::fixture::astera::Astera;
use crate::fixture::cosmic_burst::CosmicBurst;
use crate::fixture::freq_strobe::FreqStrobe;
use crate::fixture::fusion_roll::FusionRoll;
use crate::fixture::group::GroupFixtureConfig;
use crate::fixture::leko::Leko;
use crate::fixture::rug_doctor::RugDoctor;
use crate::fixture::wizlet::Wizlet;

type UsedAddrs = HashMap<(UniverseIdx, usize), FixtureConfig>;

#[derive(Default)]
pub struct Patch {
    fixtures: HashMap<FixtureGroupKey, FixtureGroup>,
    fixture_type_lookup: HashMap<&'static str, FixtureType>,
    used_addrs: UsedAddrs,
}

lazy_static! {
    static ref PATCHERS: Vec<Patcher> = vec![
        Astera::patcher(),
        Astroscan::patcher(),
        Aquarius::patcher(),
        Color::patcher(),
        Colordynamic::patcher(),
        Comet::patcher(),
        CosmicBurst::patcher(),
        Dimmer::patcher(),
        Faderboard::patcher(),
        FreedomFries::patcher(),
        FreqStrobe::patcher(),
        FusionRoll::patcher(),
        H2O::patcher(),
        Hypnotic::patcher(),
        Leko::patcher(),
        Lumasphere::patcher(),
        Radiance::patcher(),
        RotosphereQ3::patcher(),
        RushWizard::patcher(),
        RugDoctor::patcher(),
        SolarSystem::patcher(),
        Starlight::patcher(),
        UvLedBrick::patcher(),
        Venus::patcher(),
        WizardExtreme::patcher(),
        Wizlet::patcher(),
    ];
}

fn get_candidate(name: &str, options: &Options) -> Result<PatchCandidate> {
    let mut candidates = PATCHERS
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

impl Patch {
    pub fn patch(
        &mut self,
        channels: &mut Channels,
        cfg: FixtureGroupConfig,
    ) -> anyhow::Result<()> {
        let candidate = get_candidate(&cfg.name, &cfg.options)?;
        for fixture_cfg in cfg.fixture_configs(candidate.channel_count) {
            self.patch_one(channels, fixture_cfg)?;
        }
        Ok(())
    }

    /// Patch a single fixture config.
    fn patch_one(&mut self, channels: &mut Channels, cfg: FixtureConfig) -> anyhow::Result<()> {
        let candidate = get_candidate(&cfg.name, &cfg.options)?;
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
                mirror: cfg.mirror,
            });
            return Ok(());
        }
        // No existing group; create a new one.
        cfg.channel.then(|| channels.add(key.clone()));

        let group = FixtureGroup::new(
            key.clone(),
            GroupFixtureConfig {
                universe: cfg.universe,
                dmx_addr: cfg.addr.map(|a| a.dmx_index()),
                mirror: cfg.mirror,
            },
            candidate.channel_count,
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
                Ok(fixture) => Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(),
                    fixture: Box::new(fixture),
                })),
                Err(e) => Some(Err(e)),
            }
        })
    }

    /// The number of contiguous DMX channels used by the fixture.
    fn channel_count(&self) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    fn new(_options: &Options) -> Result<Self> {
        Ok(Self::default())
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
                Ok(fixture) => Some(Ok(PatchCandidate {
                    fixture_type: Self::NAME,
                    channel_count: fixture.channel_count(),
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
    fn channel_count(&self) -> usize;

    /// Create a new instance of the fixture from the provided options.
    /// Non-customizable fixtures will fall back to using default.
    /// This can be overridden for fixtures that are customizable.
    fn new(_options: &Options) -> Result<Self> {
        Ok(Self::default())
    }
}
