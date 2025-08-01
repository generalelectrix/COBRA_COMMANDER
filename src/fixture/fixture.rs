//! Types related to specifying and controlling individual fixture models.
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::time::Duration;

use anyhow::{bail, Result};
use number::Phase;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

use super::animation_target::{
    ControllableTargetedAnimation, TargetedAnimationValues, TargetedAnimations, N_ANIM,
};
use super::FixtureGroupControls;
use crate::channel::ChannelControlMessage;
use crate::fixture::animation_target::AnimationTarget;
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};

/// Statically-defined fixture type name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FixtureType(pub &'static str);

impl Deref for FixtureType {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl Display for FixtureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

// NOTE: do NOT derive Serialize or Deserialize for this type! We do not want
// instances of this to end up escaping the process, since these identifiers
// would not be stable if the state of the show were written to disk and then
// later loaded by a version of the software with a fixture patch that has
// re-ordered, re-numbered, or otherwise messed with the render modes.
//
// This entire mechanism is a hack around the fact that each fixture group
// maintains a single fixture model, but we may want to render that same model
// to fixtures that require a different rendering mode.
/// Index of multiple render modes for fixtures that support them.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub struct RenderMode(pub u8);

/// Provide helper methods to a enum that represents different fixture rendering modes.
///
/// The strum VariantArray trait is used to refer to variants by index.
pub trait EnumRenderModel: VariantArray + PartialEq + Debug + Clone {
    /// Get the render mode index for this model.
    fn render_mode(&self) -> RenderMode {
        assert!(Self::VARIANTS.len() <= 255);
        RenderMode(Self::VARIANTS.iter().position(|m| m == self).unwrap() as u8)
    }

    /// Get the render model referred to by the provided render mode.
    ///
    /// Return an error if render_mode is None or out of range.
    fn model_for_mode(render_mode: Option<RenderMode>) -> Result<Self> {
        let Some(render_mode) = render_mode else {
            bail!("missing render mode for {}", std::any::type_name::<Self>());
        };
        let Some(model) = Self::VARIANTS.get(render_mode.0 as usize) else {
            bail!(
                "render mode {} is out of range for {}",
                render_mode.0,
                std::any::type_name::<Self>()
            );
        };
        Ok(model.clone())
    }
}

pub trait EmitState {
    /// Emit the current state of all controls.
    fn emit_state(&self, emitter: &FixtureStateEmitter);
}

pub trait Control {
    /// Process the provided OSC control message.
    ///
    /// Return true if the control message was handled.
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool>;

    /// Process a channel control message, if the fixture uses it.
    #[allow(unused)]
    fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        // Ignore channel control messages by default.
        Ok(false)
    }
}

pub trait ControllableFixture: EmitState + Control {
    #[allow(unused)]
    fn update(&mut self, master_controls: &MasterControls, dt: Duration) {}
}

pub trait NonAnimatedFixture: ControllableFixture {
    /// Render into the provided DMX buffer.
    /// The buffer will be pre-sized to the fixture's channel count and offset
    /// to the fixture's start address.
    /// The master controls are provided to potentially alter the render process.
    fn render(&self, group_controls: &FixtureGroupControls, dmx_buffer: &mut [u8]);
}

pub trait AnimatedFixture: ControllableFixture {
    type Target: AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    );
}

pub trait Fixture: ControllableFixture {
    /// Render into the provided DMX buffer.
    /// The buffer will be pre-sized to the fixture's channel count and offset
    /// to the fixture's start address.
    /// Control parameters specific to an individual fixture in the group are
    /// provided.
    /// An animation phase offset is provided.
    fn render(
        &self,
        phase_offset: Phase,
        offset_index: usize,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    );

    /// Get the animation with the provided index.
    fn get_animation(&self, index: usize) -> Option<&dyn ControllableTargetedAnimation>;

    /// Get the animation with the provided index, mutably.
    fn get_animation_mut(&mut self, index: usize)
        -> Option<&mut dyn ControllableTargetedAnimation>;
}

impl<T> Fixture for T
where
    T: NonAnimatedFixture,
{
    fn render(
        &self,
        _phase_offset: Phase,
        _offset_index: usize,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    ) {
        self.render(group_controls, dmx_buffer)
    }

    fn get_animation_mut(
        &mut self,
        _index: usize,
    ) -> Option<&mut dyn ControllableTargetedAnimation> {
        None
    }

    fn get_animation(&self, _index: usize) -> Option<&dyn ControllableTargetedAnimation> {
        None
    }
}

#[derive(Debug)]
pub struct FixtureWithAnimations<F: AnimatedFixture> {
    pub fixture: F,
    pub animations: TargetedAnimations<F::Target>,
}

impl<F: AnimatedFixture> EmitState for FixtureWithAnimations<F> {
    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        self.fixture.emit_state(emitter);
    }
}

impl<F: AnimatedFixture> Control for FixtureWithAnimations<F> {
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        self.fixture.control(msg, emitter)
    }

    fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        self.fixture.control_from_channel(msg, emitter)
    }
}

impl<F: AnimatedFixture> ControllableFixture for FixtureWithAnimations<F> {
    fn update(&mut self, master_controls: &MasterControls, dt: Duration) {
        self.fixture.update(master_controls, dt);
        for ta in &mut self.animations {
            ta.animation
                .update_state(dt, master_controls.audio_envelope);
        }
    }
}

impl<F: AnimatedFixture> Fixture for FixtureWithAnimations<F> {
    fn render(
        &self,
        phase_offset: Phase,
        offset_index: usize,
        group_controls: &FixtureGroupControls,
        dmx_buffer: &mut [u8],
    ) {
        let mut animation_vals = [(0.0, F::Target::default()); N_ANIM];
        // FIXME: implement unipolar variant of animations
        for (i, ta) in self.animations.iter().enumerate() {
            animation_vals[i] = (
                ta.animation.get_value(
                    phase_offset,
                    offset_index,
                    &group_controls.master_controls.clock_state,
                    group_controls.master_controls.audio_envelope,
                ),
                ta.target,
            );
        }
        self.fixture.render_with_animations(
            group_controls,
            TargetedAnimationValues(&animation_vals),
            dmx_buffer,
        );
    }

    fn get_animation_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut dyn ControllableTargetedAnimation> {
        let animation = self.animations.get_mut(index)?;
        Some(&mut *animation)
    }

    fn get_animation(&self, index: usize) -> Option<&dyn ControllableTargetedAnimation> {
        let animation = self.animations.get(index)?;
        Some(animation)
    }
}
