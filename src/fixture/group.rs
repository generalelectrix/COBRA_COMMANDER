//! Define groups of fixtures, sharing a common fixture

use anyhow::{ensure, Context};
use color_organ::ColorOrganHsluv;
use color_organ::FixtureId;
use log::error;
use std::fmt::{Debug, Display};
use std::time::Duration;

use log::debug;
use number::Phase;

use super::animation_target::ControllableTargetedAnimation;
use super::fixture::{Fixture, FixtureType, RenderMode};
use super::prelude::ChannelStateEmitter;
use crate::channel::ChannelControlMessage;
use crate::color::Hsluv;
use crate::config::FixtureGroupKey;
use crate::config::Options;
use crate::dmx::DmxBuffer;
use crate::fixture::FixtureGroupControls;
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};
use crate::preview::Previewer;

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
    /// The group options that were used to construct the fixture.
    ///
    /// These are retained to determine if we need to re-initialize a group when
    /// repatching.
    options: Options,
    /// Is strobing enabled for this fixture?
    /// FIXME: it feels a bit odd to have group-level controllable parameters.
    /// This might be a side effect of not having a data structure that
    /// represents "channel state".
    strobe_enabled: bool,
}

impl FixtureGroup {
    /// Create empty fixture group from an initialized fixture model.
    pub fn empty(
        fixture_type: FixtureType,
        key: FixtureGroupKey,
        fixture: Box<dyn Fixture>,
        options: Options,
    ) -> Self {
        Self {
            fixture_type,
            key,
            fixture_configs: vec![],
            color_organ: None,
            fixture,
            options,
            strobe_enabled: false,
        }
    }

    /// Reconfigure this group using the state from another group, if compatible.
    ///
    /// Return true if we performed reconfiguration.
    ///
    /// This allows maintaining mutable fixture state when repatching, so control
    /// parameters do not change if the patch for this group is compatible.
    pub fn reconfigure_from(&mut self, other: FixtureGroup) -> bool {
        if self.fixture_type != other.fixture_type || self.options != other.options {
            return false;
        }
        self.fixture = other.fixture;
        self.strobe_enabled = other.strobe_enabled;
        true
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

    /// Reset all of the animations in this group.
    pub fn reset_animations(&mut self) {
        self.fixture.reset_animations();
    }

    pub fn fixture_configs(&self) -> &[GroupFixtureConfig] {
        &self.fixture_configs
    }

    /// Emit the current state of all controls.
    pub fn emit_state(&self, emitter: ChannelStateEmitter) {
        emitter.emit(crate::channel::ChannelStateChange::Strobe(
            self.strobe_enabled,
        ));
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
        let emitter = &FixtureStateEmitter::new(&self.key, channel_emitter);
        if matches!(msg, ChannelControlMessage::ToggleStrobe) {
            // If the fixture can't strobe, ignore the control.
            if self.fixture.strobe_mode().is_none() {
                return Ok(true);
            }
            self.strobe_enabled = !self.strobe_enabled;
            emitter.emit_channel(crate::channel::ChannelStateChange::Strobe(
                self.strobe_enabled,
            ));
            return Ok(true);
        }
        self.fixture.control_from_channel(msg, emitter)
    }

    /// The master controls are provided to potentially alter the update.
    pub fn update(&mut self, master_controls: &MasterControls, delta_t: Duration) {
        self.fixture.update(master_controls, delta_t);
        if let Some(color_organ) = &mut self.color_organ {
            color_organ.update(delta_t);
        }
    }

    /// Render into the provided DMX universe.
    /// The master controls are provided to potentially alter the render.
    pub fn render(
        &self,
        master_controls: &MasterControls,
        dmx_buffers: &mut [DmxBuffer],
        preview: &Previewer,
    ) {
        let phase_offset_per_fixture = Phase::new(1.0 / self.fixture_configs.len() as f64);
        let group_name = self.qualified_name();
        let preview = preview.for_group(&group_name);
        for (i, cfg) in self.fixture_configs.iter().enumerate() {
            let Some(dmx_index) = cfg.dmx_index else {
                continue;
            };
            let phase_offset = phase_offset_per_fixture * i as f64;
            let Some(dmx_univ_buf) = dmx_buffers.get_mut(cfg.universe) else {
                error!(
                    "Universe index {} for patch {i} of {} is out of range.",
                    cfg.universe,
                    self.qualified_name(),
                );
                continue;
            };
            let dmx_buf = &mut dmx_univ_buf[dmx_index..dmx_index + cfg.channel_count];
            self.fixture.render(
                phase_offset,
                i,
                &FixtureGroupControls {
                    master_controls,
                    mirror: cfg.mirror,
                    render_mode: cfg.render_mode,
                    color: self.color_organ.as_ref().and_then(|color_organ| {
                        color_organ.render(FixtureId(i as u32)).map(|color| Hsluv {
                            hue: color.hue,
                            sat: color.saturation,
                            lightness: color.lightness,
                        })
                    }),
                    strobe_enabled: self.strobe_enabled,
                    preview: &preview,
                },
                dmx_buf,
            );
            debug!("{}@{}: {:?}", self.qualified_name(), dmx_index + 1, dmx_buf);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GroupFixtureConfig {
    /// The starting index into the DMX buffer for a fixture in a group.
    /// This is a buffer index - as in, indexed from 0, not 1.
    ///
    /// If None, the fixture is assumed to not render to DMX.
    pub dmx_index: Option<usize>,
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
