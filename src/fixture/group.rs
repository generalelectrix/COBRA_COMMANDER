//! Define groups of fixtures, sharing a common fixture

use anyhow::{Context, ensure};
use color_organ::ColorOrganHsluv;
use color_organ::FixtureId;
use log::error;
use std::fmt::{Debug, Display};
use std::time::Duration;

use log::debug;
use number::Phase;

use super::animation_target::ControllableTargetedAnimation;
use super::control::OscControlDescription;
use super::fixture::{Fixture, FixtureType, RenderMode};
use super::prelude::ChannelStateEmitter;
use crate::channel::ChannelControlMessage;
use crate::color::Hsluv;
use crate::config::GroupId;
use crate::config::GroupName;
use crate::config::Options;
use crate::dmx::DmxUniverse;
use crate::fixture::FixtureGroupControls;
use crate::fixture::fixture::FixtureGroupUpdate;
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};
use crate::positioner::Positioner;
use crate::preview::Previewer;
use crate::strobe::FlashState;
use crate::strobe::StrobeResponse;

pub struct FixtureGroup {
    /// Stable UUID-based identity for this group.
    id: GroupId,
    /// The fixture type of this group.
    fixture_type: FixtureType,
    /// Human-readable name for this group. Used in OSC addresses.
    /// May change across repatches; stable identity is `id`, not this.
    name: GroupName,
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
    /// Is strobing enabled for this group?
    /// FIXME: it feels a bit odd to have group-level controllable parameters.
    /// This might be a side effect of not having a data structure that
    /// represents "channel state".
    strobe_enabled: bool,
    /// Current strobe flash state for this group. If the fixture cannot strobe,
    /// this will be None.
    flash_state: Option<FlashState>,
    /// Per-group positioner state. `Some` iff this group's fixture type
    /// supports the positioner.
    positioner: Option<Positioner>,
}

impl FixtureGroup {
    /// Create empty fixture group from an initialized fixture model.
    pub fn empty(
        id: GroupId,
        fixture_type: FixtureType,
        name: GroupName,
        fixture: Box<dyn Fixture>,
        strobe_response: Option<StrobeResponse>,
        options: Options,
    ) -> Self {
        Self {
            strobe_enabled: false,
            flash_state: strobe_response.map(FlashState::new),
            id,
            fixture_type,
            name,
            fixture_configs: vec![],
            color_organ: None,
            fixture,
            options,
            positioner: None,
        }
    }

    /// Universally-stable identity for this group.
    pub fn id(&self) -> GroupId {
        self.id
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
        // Positioner state survives a repatch when the fixture type and
        // options match. If the new patch has a different fixture count,
        // resize each preset's per-fixture offset vector (zero-padding on
        // growth, truncating tail entries on shrinkage).
        if let Some(mut positioner) = other.positioner {
            positioner.reconcile_to_fixture_count(self.fixture_configs.len());
            self.positioner = Some(positioner);
        }
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

    /// Seed a positioner for this group, sized to its current fixture count,
    /// if the fixture type supports the positioner. No-op otherwise. Call
    /// after all `patch` calls for the group have run, so the offset vectors
    /// are sized correctly.
    pub fn init_positioner_if_supported(&mut self) {
        if self.fixture.supports_positioner() {
            self.positioner = Some(Positioner::default_for(self.fixture_configs.len()));
        }
    }

    /// Get a mutable reference to the group's color organ, if in use.
    pub fn color_organ_mut(&mut self) -> Option<&mut ColorOrganHsluv> {
        self.color_organ.as_mut()
    }

    /// Return a struct that can write the qualified name of this group.
    ///
    /// This will be just the fixture type name if the group name is identical.
    /// Otherwise, it will be the group name followed by the fixture type in
    /// parentheses.
    pub fn qualified_name(&self) -> FixtureGroupQualifiedNameFormatter<'_> {
        FixtureGroupQualifiedNameFormatter {
            fixture_type: self.fixture_type,
            name: &self.name.0,
        }
    }

    #[cfg_attr(not(test), expect(unused))]
    /// Return descriptions of all OSC controls this fixture exposes.
    pub fn describe_controls(&self) -> Vec<OscControlDescription> {
        self.fixture.describe_controls()
    }

    pub fn strobe_enabled(&self) -> bool {
        self.strobe_enabled
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

    /// Read-only access to the positioner, if this group is positionable.
    pub fn positioner(&self) -> Option<&crate::positioner::Positioner> {
        self.positioner.as_ref()
    }

    /// Mutable access to the positioner, if this group is positionable.
    #[cfg(test)]
    pub fn positioner_mut(&mut self) -> Option<&mut crate::positioner::Positioner> {
        self.positioner.as_mut()
    }

    /// The group's name and a mutable handle to its positioner, borrowed as
    /// disjoint fields so both can be held simultaneously.
    pub fn split_for_positioner_dispatch(
        &mut self,
    ) -> (&GroupName, Option<&mut crate::positioner::Positioner>) {
        (&self.name, self.positioner.as_mut())
    }

    /// Emit the current state of all controls.
    pub fn emit_state(&self, emitter: ChannelStateEmitter) {
        emitter.emit(crate::channel::ChannelStateChange::Strobe(
            self.strobe_enabled,
        ));
        let fixture_emitter = FixtureStateEmitter::new(&self.name, emitter);
        // Per-group positioner state (preset radio + label array). Scoped to
        // /{group_name}/... via the FixtureStateEmitter that already wraps
        // the address with the group name.
        if let Some(positioner) = &self.positioner {
            positioner.emit_per_group_state(&fixture_emitter);
        }
        self.fixture.emit_state(&fixture_emitter);
    }

    /// Process the provided control message.
    pub fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: ChannelStateEmitter,
    ) -> anyhow::Result<()> {
        let fixture_emitter = FixtureStateEmitter::new(&self.name, emitter);
        let fixture_count = self.fixture_configs.len();

        // Try positioner first so per-group `PositionPreset*` messages don't
        // fall through to the fixture's own dispatch. Returns `None` for
        // anything that isn't a positioner control; the fixture handles the
        // rest.
        if let Some(positioner) = self.positioner.as_mut()
            && let Some(result) =
                positioner.control_osc_per_group(msg, fixture_count, &fixture_emitter)
        {
            return result.with_context(|| self.qualified_name().to_string());
        }

        let handled = self
            .fixture
            .control(msg, &fixture_emitter)
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
        let emitter = &FixtureStateEmitter::new(&self.name, channel_emitter);
        if matches!(msg, ChannelControlMessage::ToggleStrobe) {
            // If the fixture can't strobe, ignore the control.
            if self.flash_state.is_none() {
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
    pub fn update(&mut self, update: FixtureGroupUpdate, delta_t: Duration) {
        self.fixture.update(update, delta_t);
        if let Some(color_organ) = &mut self.color_organ {
            color_organ.update(delta_t);
        }
        if let Some(fs) = &mut self.flash_state {
            if update.flash_now {
                fs.flash_now();
            } else {
                fs.update(1);
            }
        }
    }

    /// Render into the provided DMX universe.
    /// The master controls are provided to potentially alter the render.
    pub fn render(
        &self,
        master_controls: &MasterControls,
        dmx: &mut [DmxUniverse],
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
            let Some(dmx_univ) = dmx.get_mut(cfg.universe) else {
                error!(
                    "{}",
                    crate::fixture::patch::patch_inconsistency(
                        "PI-004",
                        format!(
                            "render: fixture {i} of {} requested universe {} but only {} are available",
                            self.qualified_name(),
                            cfg.universe,
                            dmx.len(),
                        ),
                    )
                );
                continue;
            };
            let dmx_buf = &mut dmx_univ.buffer[dmx_index..dmx_index + cfg.channel_count];
            let positioner_offset = self.positioner.as_ref().and_then(|p| {
                p.presets
                    .get(p.active)
                    .and_then(|preset| preset.offsets.get(i))
                    .copied()
            });
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
                    flash_on: self
                        .flash_state
                        .as_ref()
                        .map(FlashState::is_on)
                        .unwrap_or_default(),
                    preview: &preview,
                    positioner_offset,
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
    name: &'a str,
}

impl<'a> Display for FixtureGroupQualifiedNameFormatter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name == self.fixture_type.0 {
            f.write_str(&self.fixture_type)
        } else {
            write!(f, "{}({})", self.name, self.fixture_type)
        }
    }
}
