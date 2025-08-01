//! Maintain UI state for animations.
use anyhow::bail;
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
            ControlMessage::Target(msg) => {
                let Some(anim) = self.current_animation(channel, group) else {
                    // Selected group is not animated. Ignore.
                    return Ok(());
                };
                if anim.target() == msg {
                    return Ok(());
                }
                anim.set_target(msg)?;
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

    fn animation_index_for_channel(&self, channel: ChannelId) -> usize {
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

struct InnerAnimationEmitter<'a>(&'a dyn EmitScopedControlMessage);

impl<'a> EmitAnimationStateChange for InnerAnimationEmitter<'a> {
    fn emit_animation_state_change(&mut self, sc: tunnels::animation::StateChange) {
        AnimationUIState::emit_osc_state_change(StateChange::Animation(sc), self.0);
    }
}

#[derive(Clone, Debug)]
pub enum ControlMessage {
    Animation(tunnels::animation::ControlMessage),
    Target(AnimationTargetIndex),
    SelectAnimation(usize),
    Copy,
    Paste,
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
