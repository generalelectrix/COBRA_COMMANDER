use anyhow::{ensure, Result};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display};
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use lazy_static::lazy_static;
use log::{debug, info};
use number::{Phase, UnipolarFloat};
use serde::{Deserialize, Serialize};

use self::animation_target::{
    ControllableTargetedAnimation, TargetedAnimation, TargetedAnimationValues,
};
use self::aquarius::{
    Aquarius, ControlMessage as AquariusControlMessage, StateChange as AquariusStateChange,
};
use self::astroscan::{
    Astroscan, ControlMessage as AstroscanControlMessage, StateChange as AstroscanStateChange,
};
use self::color::{Color, ControlMessage as ColorControlMessage, StateChange as ColorStateChange};
use self::colordynamic::{
    ControlMessage as ColordynamicControlMessage, StateChange as ColordynamicStateChange,
};
use self::comet::{Comet, ControlMessage as CometControlMessage, StateChange as CometStateChange};
use self::dimmer::{
    ControlMessage as DimmerControlMessage, Dimmer, StateChange as DimmerStateChange,
};
use self::faderboard::{
    ControlMessage as FaderboardControlMessage, Faderboard, StateChange as FaderboardStateChange,
};
use self::freedom_fries::{
    ControlMessage as FreedomFriesControlMessage, FreedomFries,
    StateChange as FreedomFriesStateChange,
};
use self::h2o::{ControlMessage as H2OControlMessage, StateChange as H2OStateChange, H2O};
use self::hypnotic::{
    ControlMessage as HypnoticControlMessage, Hypnotic, StateChange as HypnoticStateChange,
};
use self::lumasphere::{
    ControlMessage as LumasphereControlMessage, Lumasphere, StateChange as LumasphereStateChange,
};
use self::radiance::{
    ControlMessage as RadianceControlMessage, Radiance, StateChange as RadianceStateChange,
};
use self::rotosphere_q3::{
    ControlMessage as RotosphereQ3ControlMessage, RotosphereQ3,
    StateChange as RotosphereQ3StateChange,
};
use self::rush_wizard::{
    ControlMessage as RushWizardControlMessage, RushWizard, StateChange as RushWizardStateChange,
};
use self::solar_system::{
    ControlMessage as SolarSystemControlMessage, SolarSystem, StateChange as SolarSystemStateChange,
};
use self::starlight::{
    ControlMessage as StarlightControlMessage, Starlight, StateChange as StarlightStateChange,
};
use self::swarmolon::{
    ControlMessage as SwarmolonControlMessage, StateChange as SwarmolonStateChange, Swarmolon,
};
use self::uv_led_brick::{
    ControlMessage as UvLedBrickControlMessage, StateChange as UvLedBrickStateChange, UvLedBrick,
};
use self::venus::{ControlMessage as VenusControlMessage, StateChange as VenusStateChange, Venus};
use self::wizard_extreme::{
    ControlMessage as WizardExtremeControlMessage, StateChange as WizardExtremeStateChange,
    WizardExtreme,
};
use crate::animation::{
    ControlMessage as AnimationControlMessage, GroupSelection, StateChange as AnimationStateChange,
};
use crate::config::{FixtureConfig, Options};
use crate::dmx::{DmxBuffer, UniverseIdx};
use crate::fixture::animation_target::AnimationTarget;
use crate::fixture::colordynamic::Colordynamic;
use crate::master::{
    Autopilot, ControlMessage as MasterControlMessage, MasterControls,
    StateChange as MasterStateChange, Strobe,
};
use crate::osc::MapControls;

pub mod animation_target;
pub mod aquarius;
pub mod astroscan;
pub mod color;
pub mod colordynamic;
pub mod comet;
pub mod dimmer;
pub mod faderboard;
pub mod freedom_fries;
pub mod generic;
pub mod h2o;
pub mod hypnotic;
pub mod lumasphere;
pub mod radiance;
pub mod rotosphere_q3;
pub mod rush_wizard;
pub mod solar_system;
pub mod starlight;
pub mod swarmolon;
pub mod uv_led_brick;
pub mod venus;
pub mod wizard_extreme;

/// Identify a named group of a particular type of fixture.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct GroupName(Option<Arc<String>>);

impl GroupName {
    pub fn none() -> Self {
        Self(None)
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    pub fn new<S: Into<String>>(v: S) -> Self {
        Self(Some(Arc::new(v.into())))
    }

    pub fn inner(&self) -> &Option<Arc<String>> {
        &self.0
    }
}

impl From<&Option<String>> for GroupName {
    fn from(v: &Option<String>) -> Self {
        match v {
            None => Self::none(),
            Some(v) => Self::new(v),
        }
    }
}

impl Display for GroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0.as_ref().map(|g| g.as_str()).unwrap_or("none")
        )
    }
}

/// Uniquely identify a specific fixture group.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct FixtureGroupKey {
    pub fixture: Cow<'static, str>,
    pub group: GroupName,
}

impl Display for FixtureGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.group, self.fixture)
    }
}

pub trait EmitStateChange {
    fn emit(&mut self, sc: StateChange);
}

pub trait EmitFixtureStateChange {
    fn emit(&mut self, sc: FixtureStateChange);

    fn emit_astroscan(&mut self, sc: AstroscanStateChange) {
        self.emit(FixtureStateChange::Astroscan(sc));
    }

    fn emit_comet(&mut self, sc: CometStateChange) {
        self.emit(FixtureStateChange::Comet(sc));
    }

    fn emit_colordynamic(&mut self, sc: ColordynamicStateChange) {
        self.emit(FixtureStateChange::Colordynamic(sc));
    }

    fn emit_lumasphere(&mut self, sc: LumasphereStateChange) {
        self.emit(FixtureStateChange::Lumasphere(sc));
    }

    fn emit_venus(&mut self, sc: VenusStateChange) {
        self.emit(FixtureStateChange::Venus(sc));
    }

    fn emit_h2o(&mut self, sc: H2OStateChange) {
        self.emit(FixtureStateChange::H2O(sc));
    }

    fn emit_hypnotic(&mut self, sc: HypnoticStateChange) {
        self.emit(FixtureStateChange::Hypnotic(sc));
    }

    fn emit_aquarius(&mut self, sc: AquariusStateChange) {
        self.emit(FixtureStateChange::Aquarius(sc));
    }

    fn emit_radiance(&mut self, sc: RadianceStateChange) {
        self.emit(FixtureStateChange::Radiance(sc));
    }

    fn emit_swarmolon(&mut self, sc: SwarmolonStateChange) {
        self.emit(FixtureStateChange::Swarmolon(sc));
    }

    fn emit_rotosphere_q3(&mut self, sc: RotosphereQ3StateChange) {
        self.emit(FixtureStateChange::RotosphereQ3(sc));
    }

    fn emit_freedom_fries(&mut self, sc: FreedomFriesStateChange) {
        self.emit(FixtureStateChange::FreedomFries(sc));
    }

    fn emit_faderboard(&mut self, sc: FaderboardStateChange) {
        self.emit(FixtureStateChange::Faderboard(sc));
    }

    fn emit_rush_wizard(&mut self, sc: RushWizardStateChange) {
        self.emit(FixtureStateChange::RushWizard(sc));
    }

    fn emit_wizard_extreme(&mut self, sc: WizardExtremeStateChange) {
        self.emit(FixtureStateChange::WizardExtreme(sc));
    }

    fn emit_solar_system(&mut self, sc: SolarSystemStateChange) {
        self.emit(FixtureStateChange::SolarSystem(sc));
    }

    fn emit_color(&mut self, sc: ColorStateChange) {
        self.emit(FixtureStateChange::Color(sc));
    }

    fn emit_dimmer(&mut self, sc: DimmerStateChange) {
        self.emit(FixtureStateChange::Dimmer(sc));
    }

    fn emit_uv_led_brick(&mut self, sc: UvLedBrickStateChange) {
        self.emit(FixtureStateChange::UvLedBrick(sc));
    }

    fn emit_starlight(&mut self, sc: StarlightStateChange) {
        self.emit(FixtureStateChange::Starlight(sc));
    }
}

#[derive(Debug)]
pub struct StateChange {
    pub group: GroupName,
    pub sc: FixtureStateChange,
}

#[derive(Clone, Debug)]
pub enum FixtureStateChange {
    Astroscan(AstroscanStateChange),
    Comet(CometStateChange),
    Lumasphere(LumasphereStateChange),
    Venus(VenusStateChange),
    H2O(H2OStateChange),
    Hypnotic(HypnoticStateChange),
    Aquarius(AquariusStateChange),
    Radiance(RadianceStateChange),
    Swarmolon(SwarmolonStateChange),
    RotosphereQ3(RotosphereQ3StateChange),
    FreedomFries(FreedomFriesStateChange),
    Faderboard(FaderboardStateChange),
    RushWizard(RushWizardStateChange),
    Starlight(StarlightStateChange),
    WizardExtreme(WizardExtremeStateChange),
    SolarSystem(SolarSystemStateChange),
    Color(ColorStateChange),
    Colordynamic(ColordynamicStateChange),
    Dimmer(DimmerControlMessage),
    UvLedBrick(UvLedBrickControlMessage),
    Master(MasterStateChange),
    Animation(AnimationStateChange),
}

#[derive(Debug)]
pub struct ControlMessage {
    pub key: FixtureGroupKey,
    pub msg: FixtureControlMessage,
}

#[derive(Clone, Debug)]
pub enum FixtureControlMessage {
    Astroscan(AstroscanControlMessage),
    Comet(CometControlMessage),
    Lumasphere(LumasphereControlMessage),
    Venus(VenusControlMessage),
    H2O(H2OControlMessage),
    Hypnotic(HypnoticControlMessage),
    Aquarius(AquariusControlMessage),
    Radiance(RadianceControlMessage),
    Swarmolon(SwarmolonControlMessage),
    Starlight(StarlightControlMessage),
    RotosphereQ3(RotosphereQ3ControlMessage),
    FreedomFries(FreedomFriesControlMessage),
    Faderboard(FaderboardControlMessage),
    RushWizard(RushWizardControlMessage),
    WizardExtreme(WizardExtremeControlMessage),
    SolarSystem(SolarSystemControlMessage),
    Color(ColorControlMessage),
    Colordynamic(ColordynamicControlMessage),
    Dimmer(DimmerControlMessage),
    UvLedBrick(UvLedBrickControlMessage),
    Master(MasterControlMessage),
    RefreshUI,
    Animation(AnimationControlMessage),
    /// FIXME: horrible hack around OSC control map handlers currently being infallible
    Error(String),
}

pub const N_ANIM: usize = 4;
pub type TargetedAnimations<T> = [TargetedAnimation<T>; N_ANIM];

#[derive(Debug)]
struct GroupFixtureConfig {
    /// The starting index into the DMX buffer for a fixture in a group.
    dmx_addr: usize,
    /// The universe that this fixture is patched in.
    universe: usize,
    /// True if the fixture should be mirrored in mirror mode.
    mirror: bool,
}

#[derive(Debug)]
pub struct FixtureGroup {
    /// The unique identifier of this group.
    key: FixtureGroupKey,
    /// The configurations for the fixtures in the group.
    fixture_configs: Vec<GroupFixtureConfig>,
    /// The number of DMX channels used by this fixture.
    channel_count: usize,
    /// The inner implementation of the fixture.
    fixture: Box<dyn Fixture>,
}

impl FixtureGroup {
    pub fn key(&self) -> &FixtureGroupKey {
        &self.key
    }
    pub fn fixture_type(&self) -> &str {
        &self.key.fixture
    }

    pub fn name(&self) -> &GroupName {
        &self.key.group
    }

    pub fn get_animation(
        &mut self,
        index: usize,
    ) -> Option<&mut dyn ControllableTargetedAnimation> {
        self.fixture.get_animation(index)
    }

    pub fn is_animated(&self) -> bool {
        self.fixture.is_animated()
    }

    /// Emit the current state of all controls.
    pub fn emit_state(&self, emitter: &mut dyn EmitStateChange) {
        let mut emitter = StateChangeWithGroupEmitter {
            emitter,
            group: self.name().clone(),
        };
        self.fixture.emit_state(&mut emitter);
    }

    /// Process the provided control message.
    /// Return an error if fixture couldn't handle it.
    pub fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitStateChange,
    ) -> Result<()> {
        let mut emitter = StateChangeWithGroupEmitter {
            emitter,
            group: self.name().clone(),
        };
        let Some(bad_msg) = self.fixture.control(msg, &mut emitter) else {
            return Ok(());
        };
        bail!(
            "{} could not handle the control message {:?}",
            self.key,
            bad_msg
        );
    }

    pub fn update(&mut self, delta_t: Duration, _audio_envelope: UnipolarFloat) {
        self.fixture.update(delta_t);
    }

    /// Render into the provided DMX universe.
    /// The master controls are provided to potentially alter the render.
    pub fn render(&self, master_controls: &MasterControls, dmx_buffers: &mut [DmxBuffer]) {
        let phase_offset_per_fixture = Phase::new(1.0 / self.fixture_configs.len() as f64);
        for (i, cfg) in self.fixture_configs.iter().enumerate() {
            let phase_offset = phase_offset_per_fixture * i as f64;
            let dmx_buf =
                &mut dmx_buffers[cfg.universe][cfg.dmx_addr..cfg.dmx_addr + self.channel_count];
            self.fixture.render(
                phase_offset,
                &FixtureGroupControls {
                    master_controls,
                    mirror: cfg.mirror,
                },
                dmx_buf,
            );
            debug!("{} ({}): {:?}", self.fixture_type(), self.name(), dmx_buf);
        }
    }
}

/// Wrap a state change emitter,
struct StateChangeWithGroupEmitter<'a> {
    emitter: &'a mut dyn EmitStateChange,
    group: GroupName,
}

impl<'a> EmitFixtureStateChange for StateChangeWithGroupEmitter<'a> {
    fn emit(&mut self, sc: FixtureStateChange) {
        self.emitter.emit(StateChange {
            group: self.group.clone(),
            sc,
        });
    }
}

impl MapControls for FixtureGroup {
    fn map_controls(&self, map: &mut crate::osc::ControlMap<FixtureControlMessage>) {
        self.fixture.map_controls(map);
    }
}

type UsedAddrs = HashMap<(UniverseIdx, usize), FixtureConfig>;

#[derive(Default)]
pub struct Patch {
    fixtures: HashMap<FixtureGroupKey, FixtureGroup>,
    used_addrs: UsedAddrs,
    // Lookup from selector index to the fixture group assigned to that selector.
    selector_index: Vec<FixtureGroupKey>,
}

lazy_static! {
    static ref PATCHERS: Vec<Patcher> = vec![
        Astroscan::patcher(),
        Aquarius::patcher(),
        Color::patcher(),
        Colordynamic::patcher(),
        Comet::patcher(),
        Dimmer::patcher(),
        Faderboard::patcher(),
        FreedomFries::patcher(),
        H2O::patcher(),
        Hypnotic::patcher(),
        Lumasphere::patcher(),
        Radiance::patcher(),
        RotosphereQ3::patcher(),
        RushWizard::patcher(),
        SolarSystem::patcher(),
        Swarmolon::patcher(),
        Starlight::patcher(),
        UvLedBrick::patcher(),
        Venus::patcher(),
        WizardExtreme::patcher(),
    ];
}

impl Patch {
    pub fn patch(&mut self, cfg: FixtureConfig) -> Result<()> {
        let mut candidates = PATCHERS
            .iter()
            .flat_map(|p| p(&cfg))
            .collect::<Result<Vec<_>>>()?;
        let candidate = match candidates.len() {
            0 => bail!("unable to patch {cfg:?}"),
            1 => candidates.pop().unwrap(),
            _ => bail!(
                "multiple fixture patch candidates: {:?}",
                candidates
                    .iter()
                    .map(|c| c.fixture_type)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        self.used_addrs = self.check_collision(&candidate, &cfg)?;
        info!(
            "Controlling {} at {} (group: {}).",
            cfg.name, cfg.addr, cfg.group
        );
        let key = FixtureGroupKey {
            fixture: Cow::Borrowed(candidate.fixture_type),
            group: cfg.group,
        };
        // Either identify an existing appropriate group or create a new one.
        if let Some(group) = self.fixtures.get_mut(&key) {
            group.fixture_configs.push(GroupFixtureConfig {
                universe: cfg.universe,
                dmx_addr: cfg.addr.dmx_index(),
                mirror: cfg.mirror,
            });
            return Ok(());
        }
        // Add selector mapping index if provided.  Ensure this is an animatable fixture.
        if cfg.selector {
            ensure!(
                candidate.fixture.is_animated(),
                "cannot assign non-animatable fixture {} to a selector",
                candidate.fixture_type
            );
        }
        // No existing group; create a new one.
        if cfg.selector {
            self.selector_index.push(key.clone());
        }
        self.fixtures.insert(
            key.clone(),
            FixtureGroup {
                key,
                fixture_configs: vec![GroupFixtureConfig {
                    universe: cfg.universe,
                    dmx_addr: cfg.addr.dmx_index(),
                    mirror: cfg.mirror,
                }],
                channel_count: candidate.channel_count,
                fixture: candidate.fixture,
            },
        );

        Ok(())
    }

    /// Dynamically get the universe count.
    pub fn universe_count(&self) -> usize {
        let mut universes = HashSet::new();
        for group in self.fixtures.values() {
            for element in &group.fixture_configs {
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
        let dmx_index = cfg.addr.dmx_index();
        for addr in dmx_index..dmx_index + candidate.channel_count {
            match used_addrs.get(&(cfg.universe, addr)) {
                Some(existing_fixture) => {
                    bail!(
                        "{} at {} overlaps at DMX address {} in universe {} with {} at {}.",
                        cfg.name,
                        cfg.addr,
                        addr + 1,
                        cfg.universe,
                        existing_fixture.name,
                        existing_fixture.addr,
                    );
                }
                None => {
                    used_addrs.insert((cfg.universe, addr), cfg.clone());
                }
            }
        }
        Ok(used_addrs)
    }

    /// Get a fixture group by selector index.
    pub fn group_by_selector_mut(
        &mut self,
        selection: &GroupSelection,
    ) -> Result<&mut FixtureGroup> {
        let Some(fixture_key) = self.selector_index.get(selection.0) else {
            bail!("tried to get out-of-range selector {}.", selection.0);
        };
        if let Some(fixture) = self.fixtures.get_mut(fixture_key) {
            Ok(fixture)
        } else {
            bail!(
                "selector ID {} mapped to non-existent fixture key {fixture_key}",
                selection.0
            );
        }
    }

    /// Validate that a selector index refers to a selector that actually exists.
    pub fn validate_selector(&self, selector: usize) -> Result<GroupSelection> {
        if selector < self.selector_index.len() {
            Ok(GroupSelection(selector))
        } else {
            bail!("group selector {selector} out of range");
        }
    }

    /// Iterate over all of the labels for each selector.
    pub fn selector_labels(&self) -> impl Iterator<Item = String> + '_ {
        self.selector_index
            .iter()
            .filter_map(|i| self.fixtures.get(i))
            .map(|f| {
                if f.key.group.is_none() {
                    f.key.fixture.to_string()
                } else {
                    format!("{}({})", f.key.fixture, f.key.group)
                }
            })
    }

    /// Get the fixture patched with this key, mutably.
    pub fn get_mut(&mut self, key: &FixtureGroupKey) -> Option<&mut FixtureGroup> {
        self.fixtures.get_mut(key)
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
    fixture_type: &'static str,
    channel_count: usize,
    fixture: Box<dyn Fixture>,
}

pub type Patcher = Box<dyn Fn(&FixtureConfig) -> Option<Result<PatchCandidate>> + Sync>;

/// Fixture constructor trait to handle patching non-animating fixtures.
pub trait PatchFixture: NonAnimatedFixture + Default + 'static {
    const NAME: &'static str;

    /// Return a closure that will try to patch a fixture if it has the appropriate name.
    fn patcher() -> Patcher {
        Box::new(|cfg| {
            if cfg.name != Self::NAME {
                return None;
            }
            match Self::new(&cfg.options) {
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
    const NAME: &'static str;

    /// Return a closure that will try to patch a fixture if it has the appropriate name.
    fn patcher() -> Patcher {
        Box::new(|cfg| {
            if cfg.name != Self::NAME {
                return None;
            }
            match Self::new(&cfg.options) {
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

pub trait ControllableFixture: MapControls {
    /// Emit the current state of all controls.
    fn emit_state(&self, emitter: &mut dyn EmitFixtureStateChange);

    /// Process the provided control message.
    /// Return the message if this fixture cannot process it.
    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitFixtureStateChange,
    ) -> Option<FixtureControlMessage>;

    fn update(&mut self, _: Duration) {}
}

/// Wrap up the master and group-level controls into a single struct to pass
/// into fixtures.
pub struct FixtureGroupControls<'a> {
    /// Master controls.
    master_controls: &'a MasterControls,
    /// True if the fixture should render in mirrored mode.
    mirror: bool,
}

impl<'a> FixtureGroupControls<'a> {
    pub fn strobe(&self) -> &Strobe {
        self.master_controls.strobe()
    }

    pub fn autopilot(&self) -> &Autopilot {
        self.master_controls.autopilot()
    }
}

pub trait NonAnimatedFixture: ControllableFixture + Debug {
    /// Render into the provided DMX buffer.
    /// The buffer will be pre-sized to the fixture's channel count and offset
    /// to the fixture's start address.
    /// The master controls are provided to potentially alter the render process.
    fn render(&self, group_controls: &FixtureGroupControls, dmx_buffer: &mut [u8]);
}

pub trait AnimatedFixture: ControllableFixture + Debug {
    type Target: AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    );
}

pub trait Fixture: ControllableFixture + Debug {
    /// Render into the provided DMX buffer.
    /// The buffer will be pre-sized to the fixture's channel count and offset
    /// to the fixture's start address.
    /// The master controls are provided to potentially alter the render process.
    /// An animation phase offset is provided.
    fn render(
        &self,
        phase_offset: Phase,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    );

    /// Return true if this fixture has animations.
    fn is_animated(&self) -> bool;

    /// Get the animation with the provided index.
    fn get_animation(&mut self, index: usize) -> Option<&mut dyn ControllableTargetedAnimation>;
}

impl<T> Fixture for T
where
    T: NonAnimatedFixture,
{
    fn render(
        &self,
        _phase_offset: Phase,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    ) {
        self.render(group_controls, dmx_buffer)
    }

    fn is_animated(&self) -> bool {
        false
    }

    fn get_animation(&mut self, _index: usize) -> Option<&mut dyn ControllableTargetedAnimation> {
        None
    }
}

#[derive(Debug)]
pub struct FixtureWithAnimations<F: AnimatedFixture> {
    fixture: F,
    animations: TargetedAnimations<F::Target>,
}

impl<F: AnimatedFixture> MapControls for FixtureWithAnimations<F> {
    fn map_controls(&self, map: &mut crate::osc::ControlMap<FixtureControlMessage>) {
        self.fixture.map_controls(map)
    }
}

impl<F: AnimatedFixture> ControllableFixture for FixtureWithAnimations<F> {
    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitFixtureStateChange,
    ) -> Option<FixtureControlMessage> {
        self.fixture.control(msg, emitter)
    }

    fn emit_state(&self, emitter: &mut dyn EmitFixtureStateChange) {
        self.fixture.emit_state(emitter);
    }

    fn update(&mut self, dt: Duration) {
        self.fixture.update(dt);
        for ta in &mut self.animations {
            ta.animation.update_state(dt, UnipolarFloat::ZERO);
        }
    }
}

impl<F: AnimatedFixture> Fixture for FixtureWithAnimations<F> {
    fn render(
        &self,
        phase_offset: Phase,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    ) {
        let mut animation_vals = [(0.0, F::Target::default()); N_ANIM];
        // FIXME: implement unipolar variant of animations
        for (i, ta) in self.animations.iter().enumerate() {
            animation_vals[i] = (
                ta.animation.get_value(
                    phase_offset,
                    &group_controls.master_controls.clock_state,
                    group_controls.master_controls.audio_envelope,
                ),
                ta.target,
            );
        }
        self.fixture
            .render_with_animations(group_controls, &animation_vals, dmx_buffer);
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn get_animation(&mut self, index: usize) -> Option<&mut dyn ControllableTargetedAnimation> {
        let animation = self.animations.get_mut(index)?;
        Some(&mut *animation)
    }
}
