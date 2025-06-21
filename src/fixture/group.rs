//! Define groups of fixtures, sharing a common fixture

use anyhow::{ensure, Context};
use color_organ::ColorOrganHsluv;
use color_organ::FixtureId;
use std::borrow::Borrow;
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::time::Duration;

use log::debug;
use number::{Phase, UnipolarFloat};

use super::animation_target::ControllableTargetedAnimation;
use super::fixture::{Fixture, FixtureType, RenderMode};
use super::prelude::ChannelStateEmitter;
use crate::channel::ChannelControlMessage;
use crate::color::HsluvRenderer;
use crate::dmx::DmxBuffer;
use crate::fixture::FixtureGroupControls;
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};

pub struct FixtureGroup {
    /// The fixture type of this group.
    fixture_type: FixtureType,
    /// The unique identifier of this group. Often identical to the fixture type.
    key: FixtureGroupKey,
    /// The configurations for the fixtures in the group.
    fixture_configs: Vec<GroupFixtureConfig>,
    /// A color organ for controlling the group.
    color_organ: Option<ColorOrganHsluv>,
    /// The inner implementation of the fixture.
    fixture: Box<dyn Fixture>,
}

impl FixtureGroup {
    /// Create a fixture group, containing a single fixture config.
    pub fn new(
        fixture_type: FixtureType,
        key: FixtureGroupKey,
        fixture_config: GroupFixtureConfig,
        fixture: Box<dyn Fixture>,
    ) -> Self {
        Self {
            fixture_type,
            key,
            fixture_configs: vec![fixture_config],
            color_organ: None,
            fixture,
        }
    }

    /// Patch an additional fixture in this group.
    pub fn patch(&mut self, cfg: GroupFixtureConfig) {
        self.fixture_configs.push(cfg);
    }

    /// Initialize the color organ for this group.
    /// This should only be done after patching is complete, to ensure that
    /// we don't update the number of fixtures in the group.
    ///
    /// TODO: we might want to consider a separate FixtureGroupBuilder to uphold
    /// this invariant, but if we do that, it may make it more difficult to
    /// eventually make patching dynamic.
    pub fn use_color_organ(&mut self) {
        self.color_organ = Some(ColorOrganHsluv::new(self.fixture_configs.len()));
    }

    /// Get a mutable reference to the group's color organ, if in use.
    pub fn color_organ_mut(&mut self) -> Option<&mut ColorOrganHsluv> {
        self.color_organ.as_mut()
    }

    /// Return a struct that can write the qualified name of this group.
    ///
    /// This will be just the fixture type name if the key is identical.
    /// Otherwise, it will be the key followed by the fixture type in parentheses.
    pub fn qualified_name(&self) -> FixtureGroupQualifiedNameFormatter<'_> {
        FixtureGroupQualifiedNameFormatter {
            fixture_type: self.fixture_type,
            key: &self.key.0,
        }
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
            .with_context(|| self.qualified_name().to_string())?;
        ensure!(
            handled,
            "{} unexpectedly did not handle OSC message: {msg:?}",
            self.qualified_name()
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
        if let Some(color_organ) = &mut self.color_organ {
            color_organ.update(delta_t);
        }
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
                i,
                &FixtureGroupControls {
                    master_controls,
                    mirror: cfg.mirror,
                    render_mode: cfg.render_mode,
                    color: self.color_organ.as_ref().and_then(|color_organ| {
                        color_organ
                            .render(FixtureId(i as u32))
                            .map(|color| HsluvRenderer {
                                hue: color.hue,
                                sat: color.saturation,
                                lightness: color.lightness,
                            })
                    }),
                },
                dmx_buf,
            );
            debug!("{}: {:?}", self.qualified_name(), dmx_buf);
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
    /// Render mode index for fixtures that support more than one render mode.
    pub render_mode: Option<RenderMode>,
}

/// Uniquely identify a specific fixture group.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
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

/// Format the qualified name of a fixture group without allocating.
pub struct FixtureGroupQualifiedNameFormatter<'a> {
    fixture_type: FixtureType,
    key: &'a str,
}

impl<'a> Display for FixtureGroupQualifiedNameFormatter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key == self.fixture_type.0 {
            f.write_str(&self.fixture_type)
        } else {
            write!(f, "{}({})", self.key, self.fixture_type)
        }
    }
}
