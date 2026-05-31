//! Selection state and OSC dispatch for fixture group channels.
//!
//! Channel storage itself lives on `Patch` — this module only holds the
//! per-show selection state ("which channel is the operator focused on?") and
//! the OSC control handlers that route channel-scoped messages.

use std::fmt::Display;

use anyhow::{Result, anyhow, bail};
use log::{debug, error};
use number::{BipolarFloat, UnipolarFloat};
use serde::Deserialize;

use crate::{
    animation::AnimationUIState,
    control::EmitControlMessage,
    fixture::Patch,
    osc::{EmitOscMessage, GroupControlMap, OscControlMessage, ScopedControlEmitter},
};

/// The index of a channel within the patch's channel-bound groups.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Deserialize)]
pub struct ChannelId(usize);

impl ChannelId {
    /// Construct a `ChannelId` from a raw index. The caller is responsible for
    /// ensuring it's in range for the current patch; use
    /// [`Patch::validate_channel`] to validate untrusted indices.
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    pub fn inner(&self) -> usize {
        self.0
    }
}

impl From<ChannelId> for usize {
    fn from(value: ChannelId) -> Self {
        value.0
    }
}

impl Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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
        let current_channel = (patch.channel_count() > 0).then(|| ChannelId::new(0));
        Self {
            current_channel,
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
            _ if patch.channel_count() > 0 => Some(ChannelId::new(0)),
            _ => None,
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
                    Ok(f) => f.emit_state(ChannelStateEmitter {
                        channel_id: Some(channel_id),
                        emitter,
                    }),
                    Err(e) => error!("{e:#}"),
                }
            }
        } else {
            for (channel_id, group) in patch.channels_with_ids() {
                group.emit_state(ChannelStateEmitter {
                    channel_id: Some(channel_id),
                    emitter,
                });
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
                    ChannelStateEmitter {
                        channel_id: Some(channel_id),
                        emitter,
                    },
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

/// Provide methods to emit channel control state changes for a specific channel.
/// If no channel is set, no state change events will be emitted.
pub struct ChannelStateEmitter<'a> {
    channel_id: Option<ChannelId>,
    emitter: &'a dyn EmitControlMessage,
}

impl<'a> ChannelStateEmitter<'a> {
    /// An emitter that ignores channel state changes.
    pub fn new(channel_id: Option<ChannelId>, emitter: &'a dyn EmitControlMessage) -> Self {
        Self {
            channel_id,
            emitter,
        }
    }

    /// Emit the provided state change.
    pub fn emit(&self, msg: ChannelStateChange) {
        let Some(channel_id) = self.channel_id else {
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
    use crate::{channel::ChannelStateEmitter, control::mock::NoOpEmitter};

    pub fn no_op_emitter() -> ChannelStateEmitter<'static> {
        ChannelStateEmitter {
            channel_id: None,
            emitter: &NoOpEmitter,
        }
    }
}
