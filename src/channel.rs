//! Selection state and OSC dispatch for fixture group channels.
//!
//! Channel storage itself lives on `Patch` — this module only holds the
//! per-show selection state ("which channel is the operator focused on?") and
//! the OSC control handlers that route channel-scoped messages.

use anyhow::{Result, anyhow, bail};
use log::{debug, error};
use number::{BipolarFloat, UnipolarFloat};

use crate::{
    animation::AnimationUIState,
    control::EmitControlMessage,
    fixture::{Patch, patch::ChannelId},
    osc::{EmitOscMessage, GroupControlMap, OscControlMessage, ScopedControlEmitter},
};

/// OSC dispatch and selection state for channels.
pub struct Channels {
    /// The channel ID that is currently selected.
    current_channel: Option<ChannelId>,
    controls: GroupControlMap<ControlMessage>,
}

impl Channels {
    /// Build a `Channels` for a patch. If the patch has any channels, the
    /// first one is selected by default.
    pub fn new(patch: &Patch) -> Self {
        let mut controls = GroupControlMap::default();
        Self::map_controls(&mut controls);
        Self {
            current_channel: patch.first_channel(),
            controls,
        }
    }

    pub fn current_channel(&self) -> Option<ChannelId> {
        self.current_channel
    }

    /// Reconcile selection state against a freshly-(re)built patch. If the
    /// currently-selected channel is no longer in range, fall back to the
    /// first channel (or `None` if the patch has none).
    pub fn reconcile_to_patch(&mut self, patch: &Patch) {
        self.current_channel = match self.current_channel {
            Some(ch) if ch.inner() < patch.channel_count() => Some(ch),
            _ => patch.first_channel(),
        };
    }

    /// Emit all current channel state.
    pub fn emit_state(
        &self,
        selected_fixture_only: bool,
        patch: &Patch,
        emitter: &dyn EmitControlMessage,
    ) {
        let scoped_emitter = ScopedControlEmitter {
            entity: crate::osc::channels::GROUP,
            emitter,
        };
        if let Some(channel) = self.current_channel {
            let sc = StateChange::SelectChannel(channel);
            emitter.emit_midi_channel_message(&sc);
            Self::emit_osc_state_change(sc, &scoped_emitter);
        }
        Self::emit_osc_state_change(
            StateChange::ChannelLabels(patch.channel_labels().collect()),
            &scoped_emitter,
        );
        if selected_fixture_only {
            if let Some(channel_id) = self.current_channel {
                match patch.channel_group(channel_id) {
                    // The addressed group is the current channel by construction.
                    Ok(f) => f.emit_state(ChannelStateEmitter::new(
                        ChannelBinding::Current(channel_id),
                        emitter,
                    )),
                    Err(e) => error!("{e:#}"),
                }
            }
        } else {
            for (channel_id, group) in patch.channels_with_ids() {
                group.emit_state(ChannelStateEmitter::new(
                    ChannelBinding::resolve(Some(channel_id), self.current_channel),
                    emitter,
                ));
            }
        }
    }

    /// Handle a OSC control message.
    pub fn control_osc(
        &mut self,
        msg: &OscControlMessage,
        patch: &mut Patch,
        animation_ui: &AnimationUIState,
        emitter: &dyn EmitControlMessage,
    ) -> anyhow::Result<()> {
        let Some((ctl, _)) = self.controls.handle(msg)? else {
            return Ok(());
        };
        self.control(&ctl, patch, animation_ui, emitter)
    }

    /// Handle a typed control message.
    pub fn control(
        &mut self,
        ctl: &ControlMessage,
        patch: &mut Patch,
        animation_ui: &AnimationUIState,
        emitter: &dyn EmitControlMessage,
    ) -> anyhow::Result<()> {
        match ctl {
            ControlMessage::SelectChannel(g) => {
                let (channel, group) = patch.channel(*g)?;
                if self.current_channel == Some(channel) {
                    // Channel is not changed, ignore.
                    return Ok(());
                }
                self.current_channel = Some(channel);
                self.emit_state(true, patch, emitter);
                // FIXME this is so goddamn inside out, I hate it.
                animation_ui.emit_state(
                    channel,
                    group,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter,
                    },
                );
            }
            ControlMessage::Control { channel_id, msg } => {
                let (channel_id, group) = if let Some(id) = channel_id {
                    patch.channel_mut(*id)?
                } else {
                    let selected = self.current_channel.ok_or_else(|| {
                        anyhow!(
                            "no channel ID provided or selected for channel control message {msg:?}"
                        )
                    })?;
                    (selected, patch.channel_group_mut(selected)?)
                };
                let handled = group.control_from_channel(
                    msg,
                    ChannelStateEmitter::new(
                        ChannelBinding::resolve(Some(channel_id), self.current_channel),
                        emitter,
                    ),
                )?;
                if !handled {
                    debug!(
                        "Fixture in channel {channel_id} did not handle channel control message {msg:?}."
                    );
                }
            }
        }
        Ok(())
    }
}

/// Compute the strobe control channel index for the given channel count.
/// Returns the last fader of the last submaster wing.
pub fn strobe_control_channel(channel_count: usize) -> usize {
    (crate::midi::slots::submaster_wing_count(channel_count) * 8) - 1
}

/// The relationship between the channel an emitter is addressing and the
/// currently-selected channel.
///
/// Carried by [`ChannelStateEmitter`] so downstream code (e.g. the positioner)
/// can answer "should I update channel-scoped UI tabs?" without separately
/// threading current-channel state. Mirrors how a `channel_id` baked into the
/// emitter routes MIDI knob updates to the right hardware: the emitter's type
/// encodes the binding context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelBinding {
    /// The addressed group IS the currently-selected channel.
    Current(ChannelId),
    /// The addressed group has a channel id but isn't currently selected.
    Other(ChannelId),
    /// The addressed group isn't channel-bound at all.
    Unbound,
}

impl ChannelBinding {
    /// Resolve from the addressed group's channel id and the global
    /// current-channel selection.
    pub fn resolve(addressed: Option<ChannelId>, current: Option<ChannelId>) -> Self {
        match addressed {
            None => Self::Unbound,
            Some(id) if Some(id) == current => Self::Current(id),
            Some(id) => Self::Other(id),
        }
    }

    /// The channel id of the addressed group, if any.
    pub fn channel_id(&self) -> Option<ChannelId> {
        match self {
            Self::Current(id) | Self::Other(id) => Some(*id),
            Self::Unbound => None,
        }
    }

    /// True iff the addressed group is the currently-selected channel.
    #[expect(unused)] // Will be used by the positioner work in a follow-up.
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current(_))
    }
}

/// Provide methods to emit channel control state changes for a specific channel.
/// If no channel is set (binding is [`ChannelBinding::Unbound`]), no state
/// change events will be emitted.
pub struct ChannelStateEmitter<'a> {
    channel: ChannelBinding,
    emitter: &'a dyn EmitControlMessage,
}

impl<'a> ChannelStateEmitter<'a> {
    pub fn new(channel: ChannelBinding, emitter: &'a dyn EmitControlMessage) -> Self {
        Self { channel, emitter }
    }

    /// The channel binding this emitter is addressing.
    pub fn channel(&self) -> &ChannelBinding {
        &self.channel
    }

    /// The underlying control message sender. Useful for building sibling
    /// scoped emitters (e.g. a `/Positioner/`-scoped emitter from a
    /// `/{group_name}/`-scoped one).
    pub fn underlying(&self) -> &'a dyn EmitControlMessage {
        self.emitter
    }

    /// Emit the provided state change.
    pub fn emit(&self, msg: ChannelStateChange) {
        let Some(channel_id) = self.channel.channel_id() else {
            return;
        };
        let sc = StateChange::State { channel_id, msg };
        self.emitter.emit_midi_channel_message(&sc);
        Channels::emit_osc_state_change(
            sc,
            &ScopedControlEmitter {
                entity: crate::osc::channels::GROUP,
                emitter: self.emitter,
            },
        );
    }
}

impl<'a> EmitOscMessage for ChannelStateEmitter<'a> {
    fn emit_osc(&self, msg: rosc::OscMessage) {
        self.emitter.emit_osc(msg);
    }
}

#[derive(Clone, Debug)]
pub enum ControlMessage {
    SelectChannel(usize),
    Control {
        channel_id: Option<usize>,
        msg: ChannelControlMessage,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum ChannelControlMessage {
    Level(UnipolarFloat),
    Knob { index: KnobIndex, value: KnobValue },
    ToggleStrobe,
}

impl ChannelControlMessage {
    /// Convert this control message directly into a state change.
    ///
    /// This code path is needed in certain places that try to directly echo a
    /// channel command. We can't actually do this for button presses, because
    /// we do not assume that we can store state in a hardware button.
    pub fn as_state_change(self) -> Result<ChannelStateChange> {
        match self {
            Self::Level(v) => Ok(ChannelStateChange::Level(v)),
            Self::Knob { index, value } => Ok(ChannelStateChange::Knob { index, value }),
            Self::ToggleStrobe => {
                bail!("cannot convert a strobe toggle command into a state change")
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum StateChange {
    SelectChannel(ChannelId),
    ChannelLabels(Vec<String>),
    State {
        channel_id: ChannelId,
        msg: ChannelStateChange,
    },
    /// Clear all channel state; this makes way for a total refresh, in case
    /// the number of valid channels has changed.
    Clear,
}

pub type KnobIndex = u8;

#[derive(Clone, Copy, Debug)]
pub enum ChannelStateChange {
    Level(UnipolarFloat),
    Knob { index: KnobIndex, value: KnobValue },
    Strobe(bool),
}

#[derive(Clone, Copy, Debug)]
pub enum KnobValue {
    Unipolar(UnipolarFloat),
    Bipolar(BipolarFloat),
}

impl KnobValue {
    /// Return this knob value as a unipolar float.
    /// If it came in as a bipolar knob, assume the entire knob range should
    /// be mapped to the unipolar range, such that a "centered" bipolar knob
    /// becomes a unipolar knob at 0.5.
    pub fn as_unipolar(&self) -> UnipolarFloat {
        match self {
            Self::Unipolar(v) => *v,
            Self::Bipolar(v) => v.rescale_as_unipolar(),
        }
    }

    /// Return this knob value as a bipolar float.
    /// If it came in as a unipolar knob, assume the entire knob range should
    /// be mapped to the bipolar range, such that a "centered" unipolar knob
    /// becomes a bipolar knob at 0.
    pub fn as_bipolar(&self) -> BipolarFloat {
        match self {
            Self::Bipolar(v) => *v,
            Self::Unipolar(v) => v.rescale_as_bipolar(),
        }
    }
}

#[cfg(test)]
pub mod mock {
    use crate::{
        channel::{ChannelBinding, ChannelStateEmitter},
        control::mock::NoOpEmitter,
    };

    pub fn no_op_emitter() -> ChannelStateEmitter<'static> {
        ChannelStateEmitter::new(ChannelBinding::Unbound, &NoOpEmitter)
    }
}
