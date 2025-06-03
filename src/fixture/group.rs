//! Define groups of fixtures, sharing a common fixture

use anyhow::{ensure, Context};
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use log::debug;
use number::{Phase, UnipolarFloat};
use serde::{Deserialize, Serialize};

use super::animation_target::ControllableTargetedAnimation;
use super::fixture::{Fixture, FixtureType};
use super::prelude::ChannelStateEmitter;
use crate::channel::ChannelControlMessage;
use crate::dmx::DmxBuffer;
use crate::fixture::FixtureGroupControls;
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};

pub struct FixtureGroup {
    /// The unique identifier of this group.
    key: FixtureGroupKey,
    /// The configurations for the fixtures in the group.
    fixture_configs: Vec<GroupFixtureConfig>,
    /// The inner implementation of the fixture.
    fixture: Box<dyn Fixture>,
}

impl FixtureGroup {
    /// Create a fixture group, containing a single fixture config.
    pub fn new(
        key: FixtureGroupKey,
        fixture_config: GroupFixtureConfig,
        fixture: Box<dyn Fixture>,
    ) -> Self {
        Self {
            key,
            fixture_configs: vec![fixture_config],
            fixture,
        }
    }

    /// Patch an additional fixture in this group.
    pub fn patch(&mut self, cfg: GroupFixtureConfig) {
        self.fixture_configs.push(cfg);
    }

    pub fn key(&self) -> &FixtureGroupKey {
        &self.key
    }
    pub fn fixture_type(&self) -> &FixtureType {
        &self.key.fixture
    }

    pub fn name(&self) -> Option<&GroupName> {
        self.key.group.as_ref()
    }

    pub fn get_animation_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut dyn ControllableTargetedAnimation> {
        self.fixture.get_animation_mut(index)
    }

    pub fn get_animation(&self, index: usize) -> Option<&dyn ControllableTargetedAnimation> {
        self.fixture.get_animation(index)
    }

    pub fn fixture_configs(&self) -> &[GroupFixtureConfig] {
        &self.fixture_configs
    }

    /// Emit the current state of all controls.
    pub fn emit_state(&self, emitter: ChannelStateEmitter) {
        self.fixture
            .emit_state(&FixtureStateEmitter::new(&self.key, emitter));
    }

    /// Process the provided control message.
    pub fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: ChannelStateEmitter,
    ) -> anyhow::Result<()> {
        let handled = self
            .fixture
            .control(msg, &FixtureStateEmitter::new(&self.key, emitter))
            .with_context(|| self.key.clone())?;
        ensure!(
            handled,
            "{} unexpectedly did not handle OSC message: {msg:?}",
            self.key
        );
        Ok(())
    }

    /// Process the provided channel control message.
    pub fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        channel_emitter: ChannelStateEmitter,
    ) -> anyhow::Result<bool> {
        self.fixture
            .control_from_channel(msg, &FixtureStateEmitter::new(&self.key, channel_emitter))
    }

    /// The master controls are provided to potentially alter the update.
    pub fn update(
        &mut self,
        master_controls: &MasterControls,
        delta_t: Duration,
        _audio_envelope: UnipolarFloat,
    ) {
        self.fixture.update(master_controls, delta_t);
    }

    /// Render into the provided DMX universe.
    /// The master controls are provided to potentially alter the render.
    pub fn render(&self, master_controls: &MasterControls, dmx_buffers: &mut [DmxBuffer]) {
        let phase_offset_per_fixture = Phase::new(1.0 / self.fixture_configs.len() as f64);
        for (i, cfg) in self.fixture_configs.iter().enumerate() {
            let Some(dmx_addr) = cfg.dmx_addr else {
                continue;
            };
            let phase_offset = phase_offset_per_fixture * i as f64;
            let dmx_buf = &mut dmx_buffers[cfg.universe][dmx_addr..dmx_addr + cfg.channel_count];
            self.fixture.render(
                phase_offset,
                &FixtureGroupControls {
                    master_controls,
                    mirror: cfg.mirror,
                },
                dmx_buf,
            );
            debug!(
                "{} ({}): {:?}",
                self.fixture_type(),
                self.name().map(|g| g.0.as_str()).unwrap_or("none"),
                dmx_buf
            );
        }
    }
}

#[derive(Debug)]
pub struct GroupFixtureConfig {
    /// The starting index into the DMX buffer for a fixture in a group.
    ///
    /// If None, the fixture is assumed to not render to DMX.
    pub dmx_addr: Option<usize>,
    /// The universe that this fixture is patched in.
    pub universe: usize,
    /// The number of DMX channels used by this fixture.
    /// Should be set to 0 for non-DMX fixtures.
    pub channel_count: usize,
    /// True if the fixture should be mirrored in mirror mode.
    pub mirror: bool,
}

/// Uniquely identify a specific fixture group.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FixtureGroupKey {
    pub fixture: FixtureType,
    pub group: Option<GroupName>,
}

impl Display for FixtureGroupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({})",
            self.group.as_ref().map(|g| g.0.as_str()).unwrap_or("none"),
            self.fixture
        )
    }
}

/// User-provided name for a particular fixture group.
#[derive(Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct GroupName(Arc<String>);

impl GroupName {
    pub fn new<S: Into<String>>(v: S) -> Self {
        Self(Arc::new(v.into()))
    }
}

impl Deref for GroupName {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.0.as_str()
    }
}

impl Display for GroupName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
