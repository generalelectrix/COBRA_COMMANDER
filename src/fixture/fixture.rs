//! Types related to specifying and controlling individual fixture models.
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::ops::Deref;
use std::time::Duration;

use anyhow::{Result, bail};
use number::Phase;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

use super::FixtureGroupControls;
use super::animation_target::{
    AnimationSlice, ControllableTargetedAnimation, N_ANIM, TargetedAnimationValues,
    TargetedAnimations,
};
use crate::channel::ChannelControlMessage;
use crate::fixture::animation_target::AnimationTarget;
use crate::fixture::control::{DescribeOscControls, OscControlDescription};
use crate::master::MasterControls;
use crate::osc::{FixtureStateEmitter, OscControlMessage};

/// Statically-defined fixture type name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FixtureType(pub &'static str);

impl Deref for FixtureType {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl AsRef<str> for FixtureType {
    fn as_ref(&self) -> &str {
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

/// Emit controllable state back out to control surfaces.
///
/// Used for initializing as well as force-refreshing UIs.
pub trait EmitState {
    /// Emit the current state of all controls.
    fn emit_state(&self, emitter: &FixtureStateEmitter);
}

/// Process control messages from input sources.
pub trait Control {
    /// Process the provided OSC control message.
    ///
    /// Return Ok(true) if the control message was handled.
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool>;

    /// Process a channel control message, if the fixture uses it.
    ///
    /// Return Ok(true) if the control message was handled.
    fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool>;
}

/// Data scoped to a specific group to influence state update.
#[derive(Clone, Copy)]
pub struct FixtureGroupUpdate<'a> {
    pub master_controls: &'a MasterControls,
    pub flash_now: bool,
}

/// Update time-driven internal state.
pub trait Update {
    /// Update the internal state by the timestep dt.
    #[allow(unused_variables)]
    fn update(&mut self, update: FixtureGroupUpdate, dt: Duration) {}
}

pub trait NonAnimatedFixture: Update + EmitState + Control + DescribeOscControls {
    /// Render into the provided DMX buffer.
    /// The buffer will be pre-sized to the fixture's channel count and offset
    /// to the fixture's start address.
    /// The master controls are provided to potentially alter the render process.
    fn render(&self, group_controls: &FixtureGroupControls, dmx_buffer: &mut [u8]);
}

pub trait AnimatedFixture: Update + EmitState + Control + DescribeOscControls {
    type Target: AnimationTarget;

    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>;

    /// If this fixture type opts into the positioner, return the animation
    /// targets that the positioner's X, Y, and (optionally) Focus axes feed
    /// into. Default `None`: not positionable.
    ///
    /// `focus` is `Option<Self::Target>` so fixtures without a focus axis
    /// (e.g. moving-head LED washes) can opt in for pan/tilt only.
    fn positioner_axes() -> Option<crate::positioner::PositionerAxes<Self::Target>> {
        None
    }
}

pub trait Fixture: Update + EmitState + Control + DescribeOscControls {
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

    /// Reset all of the animations associated with this fixture.
    fn reset_animations(&mut self);

    /// Whether this fixture type supports the positioner. Default `false`.
    fn supports_positioner(&self) -> bool {
        false
    }
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

    fn reset_animations(&mut self) {}
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

impl<F: AnimatedFixture> DescribeOscControls for FixtureWithAnimations<F> {
    fn describe_controls(&self) -> Vec<OscControlDescription> {
        self.fixture.describe_controls()
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

impl<F: AnimatedFixture> Update for FixtureWithAnimations<F> {
    fn update(&mut self, update: FixtureGroupUpdate, dt: Duration) {
        self.fixture.update(update, dt);
        for ta in &mut self.animations {
            ta.animation
                .update_state(dt, update.master_controls.audio_envelope);
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
        // Stack buffer holding (animation_value, target) for the animation
        // contributions visible to the fixture.
        let mut anim_buf = [(0.0, F::Target::default()); N_ANIM];
        let mut anim_count = 0;
        for ta in self.animations.iter() {
            anim_buf[anim_count] = (
                ta.animation.get_value(
                    phase_offset,
                    offset_index,
                    &group_controls.master_controls.clock_state,
                    group_controls.master_controls.audio_envelope,
                ),
                ta.target,
            );
            anim_count += 1;
        }

        // Positioner contributions: 0 entries if the fixture type didn't opt in
        // or the group has no positioner offset for this fixture; 2 entries
        // (x, y) if the fixture has no focus axis; 3 if it does. The focus
        // offset is stored on every PositionOffset uniformly, but only
        // contributes to render when `axes.focus` is Some.
        let mut pos_buf = [(0.0, F::Target::default()); crate::positioner::N_POSITIONER_AXES];
        let pos_count = match (F::positioner_axes(), group_controls.positioner_offset) {
            (Some(axes), Some(off)) => {
                pos_buf[0] = (off.x.val(), axes.x);
                pos_buf[1] = (off.y.val(), axes.y);
                let mut count = 2;
                if let Some(focus_target) = axes.focus {
                    pos_buf[count] = (off.focus.val(), focus_target);
                    count += 1;
                }
                count
            }
            _ => 0,
        };

        let combined =
            AnimationSlice(&anim_buf[..anim_count]).chain(AnimationSlice(&pos_buf[..pos_count]));
        self.fixture
            .render_with_animations(group_controls, &combined, dmx_buffer);
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

    fn reset_animations(&mut self) {
        for anim in &mut self.animations {
            anim.reset();
        }
    }

    fn supports_positioner(&self) -> bool {
        F::positioner_axes().is_some()
    }
}
