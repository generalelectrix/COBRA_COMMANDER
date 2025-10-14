//! Maintain UI state for animations.
use anyhow::bail;
use log::debug;
use std::collections::HashMap;
use tunnels::animation::{Animation, EmitStateChange as EmitAnimationStateChange};

use crate::{
    control::EmitScopedControlMessage,
    fixture::{
        animation_target::{AnimationTargetIndex, ControllableTargetedAnimation, N_ANIM},
        FixtureGroup,
    },
    osc::{GroupControlMap, OscControlMessage},
    show::ChannelId,
};

pub struct AnimationUIState {
    selected_animator_by_channel: HashMap<ChannelId, usize>,
    clipboard: Animation,
    controls: GroupControlMap<ControlMessage>,
    empty_animation: EmptyAnimation,
}

impl AnimationUIState {
    pub fn new(initial_channel: Option<ChannelId>) -> Self {
        let mut controls = GroupControlMap::default();
        Self::map_controls(&mut controls);
        let mut state = Self {
            selected_animator_by_channel: Default::default(),
            clipboard: Default::default(),
            controls,
            empty_animation: Default::default(),
        };
        if let Some(channel) = initial_channel {
            state.selected_animator_by_channel.insert(channel, 0);
        }
        state
    }

    /// Emit all current animation state, including target and selection.
    pub fn emit_state(
        &self,
        channel: ChannelId,
        group: &FixtureGroup,
        emitter: &dyn EmitScopedControlMessage,
    ) {
        let (ta, index) = self
            .current_animation_with_index(channel, group)
            .unwrap_or((&self.empty_animation, 0));
        ta.anim().emit_state(&mut InnerAnimationEmitter(emitter));
        Self::emit_osc_state_change(StateChange::Target(ta.target()), emitter);
        Self::emit_osc_state_change(StateChange::SelectAnimation(index), emitter);
        Self::emit_osc_state_change(StateChange::TargetLabels(ta.target_labels()), emitter);
    }

    /// Handle a control message.
    pub fn control(
        &mut self,
        msg: ControlMessage,
        channel: ChannelId,
        group: &mut FixtureGroup,
        emitter: &dyn EmitScopedControlMessage,
    ) -> anyhow::Result<()> {
        match msg {
            ControlMessage::Animation(msg) => {
                let Some(anim) = self.current_animation(channel, group) else {
                    // Selected group is not animated. Ignore.
                    return Ok(());
                };
                anim.anim_mut()
                    .control(msg, &mut InnerAnimationEmitter(emitter));
            }
            ControlMessage::Nudge(n) => {
                let Some(anim) = self.current_animation(channel, group) else {
                    // Selected group is not animated. Ignore.
                    return Ok(());
                };
                handle_nudge(anim.anim_mut(), n, emitter);
            }
            ControlMessage::Target(msg) => {
                let Some(anim) = self.current_animation(channel, group) else {
                    // Selected group is not animated. Ignore.
                    return Ok(());
                };
                if anim.target() == msg {
                    return Ok(());
                }
                // A target index being out of range basically just means that
                // someone pushed a select button that doesn't have a target
                // assigned to it. This isn't really an error, so don't report
                // it as one.
                if let Err(err) = anim.set_target(msg) {
                    debug!("{err}");
                    return Ok(());
                }
                Self::emit_osc_state_change(StateChange::Target(msg), emitter);
            }
            ControlMessage::SelectAnimation(n) => {
                if self.animation_index_for_channel(channel) == n {
                    return Ok(());
                }
                self.set_current_animation(channel, n)?;
                self.emit_state(channel, group, emitter);
            }
            ControlMessage::Copy => {
                let Some(anim) = self.current_animation(channel, group) else {
                    return Ok(());
                };
                self.clipboard = anim.anim().clone();
            }
            ControlMessage::Paste => {
                let Some(anim) = self.current_animation(channel, group) else {
                    return Ok(());
                };
                *anim.anim_mut() = self.clipboard.clone();
                self.emit_state(channel, group, emitter);
            }
        }
        Ok(())
    }

    /// Handle a control message.
    pub fn control_osc(
        &mut self,
        msg: &OscControlMessage,
        channel: ChannelId,
        group: &mut FixtureGroup,
        emitter: &dyn EmitScopedControlMessage,
    ) -> anyhow::Result<()> {
        let Some((ctl, _)) = self.controls.handle(msg)? else {
            return Ok(());
        };
        self.control(ctl, channel, group, emitter)
    }

    fn current_animation_with_index_mut<'a>(
        &self,
        channel: ChannelId,
        group: &'a mut FixtureGroup,
    ) -> Option<(&'a mut dyn ControllableTargetedAnimation, usize)> {
        let animation_index = self.animation_index_for_channel(channel);
        let anim = group.get_animation_mut(animation_index)?;
        Some((anim, animation_index))
    }

    fn current_animation_with_index<'a>(
        &self,
        channel: ChannelId,
        group: &'a FixtureGroup,
    ) -> Option<(&'a dyn ControllableTargetedAnimation, usize)> {
        let animation_index = self.animation_index_for_channel(channel);
        let anim = group.get_animation(animation_index)?;
        Some((anim, animation_index))
    }

    fn current_animation<'a>(
        &self,
        channel: ChannelId,
        group: &'a mut FixtureGroup,
    ) -> Option<&'a mut dyn ControllableTargetedAnimation> {
        Some(self.current_animation_with_index_mut(channel, group)?.0)
    }

    pub fn animation_index_for_channel(&self, channel: ChannelId) -> usize {
        self.selected_animator_by_channel
            .get(&channel)
            .cloned()
            .unwrap_or_default()
    }

    /// Set the current animation for the current channel to the provided value.
    pub fn set_current_animation(&mut self, channel: ChannelId, n: usize) -> anyhow::Result<()> {
        if n > N_ANIM {
            bail!("animator index {n} out of range");
        }
        self.selected_animator_by_channel.insert(channel, n);
        Ok(())
    }
}

/// Handle interpreting a parameter nudge.
fn handle_nudge(anim: &mut Animation, nudge: Nudge, emitter: &dyn EmitScopedControlMessage) {
    use tunnels::animation::StateChange::*;
    let msg = tunnels::animation::ControlMessage::Set(match nudge {
        Nudge::Size(amt) => {
            let mut v = anim.size();
            v += amt;
            Size(v)
        }

        Nudge::Speed(amt) => {
            let mut v = anim.clock_speed();
            v += amt;
            Speed(v)
        }
        Nudge::DutyCycle(amt) => {
            let mut v = anim.duty_cycle();
            v += amt;
            DutyCycle(v)
        }
        Nudge::Smoothing(amt) => {
            let mut v = anim.smoothing();
            v += amt;
            Smoothing(v)
        }
        Nudge::NPeriods(amt) => {
            NPeriods(((anim.n_periods() as i32) + amt as i32).max(0).min(15) as u16)
        }
    });
    anim.control(msg, &mut InnerAnimationEmitter(emitter));
}

struct InnerAnimationEmitter<'a>(&'a dyn EmitScopedControlMessage);

impl<'a> EmitAnimationStateChange for InnerAnimationEmitter<'a> {
    fn emit_animation_state_change(&mut self, sc: tunnels::animation::StateChange) {
        AnimationUIState::emit_osc_state_change(StateChange::Animation(sc), self.0);
    }
}

#[derive(Clone, Debug)]
pub enum ControlMessage {
    Animation(tunnels::animation::ControlMessage),
    /// A "nudge" to a parameter. This handles input from controls like rotary
    /// encoders that do not store state internally, but only transmit a
    /// message when rotated. The actual amount that the nudge moves is up to
    /// the caller.
    Nudge(Nudge),
    Target(AnimationTargetIndex),
    SelectAnimation(usize),
    Copy,
    Paste,
}

/// Nudge a parameter up or down.
#[derive(Clone, Debug)]
pub enum Nudge {
    Size(f64),
    Speed(f64),
    DutyCycle(f64),
    Smoothing(f64),
    NPeriods(i16),
}

#[derive(Clone, Debug)]
pub enum StateChange {
    Animation(tunnels::animation::StateChange),
    Target(AnimationTargetIndex),
    SelectAnimation(usize),
    TargetLabels(Vec<String>),
}

#[derive(Default)]
struct EmptyAnimation(Animation);

impl ControllableTargetedAnimation for EmptyAnimation {
    fn anim(&self) -> &Animation {
        &self.0
    }

    fn anim_mut(&mut self) -> &mut Animation {
        &mut self.0
    }

    fn set_target(&mut self, _: AnimationTargetIndex) -> anyhow::Result<()> {
        Ok(())
    }

    fn target(&self) -> AnimationTargetIndex {
        0
    }

    fn target_labels(&self) -> Vec<String> {
        vec![]
    }
}
