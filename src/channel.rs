//! State and control definitions for fixture group channels.

use std::{collections::HashMap, fmt::Display};

use anyhow::{Context, Result, anyhow, bail};
use log::{debug, error};
use number::{BipolarFloat, UnipolarFloat};
use serde::Deserialize;

use crate::{
    animation::AnimationUIState,
    config::FixtureGroupKey,
    control::EmitControlMessage,
    fixture::{FixtureGroup, Patch},
    osc::{EmitOscMessage, GroupControlMap, OscControlMessage, ScopedControlEmitter},
};

/// The index of a channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Deserialize)]
pub struct ChannelId(usize);

impl ChannelId {
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

pub struct Channels {
    /// Lookup from channel index to the fixture group assigned to that channel.
    channel_index: Vec<FixtureGroupKey>,
    /// Reverse-lookup from fixture group key to channel index.
    fixture_channel_index: HashMap<FixtureGroupKey, ChannelId>,
    /// The channel ID that is currently selected.
    current_channel: Option<ChannelId>,
    controls: GroupControlMap<ControlMessage>,
}

impl Channels {
    pub fn new() -> Self {
        let mut controls = GroupControlMap::default();
        Self::map_controls(&mut controls);
        Self {
            channel_index: Default::default(),
            fixture_channel_index: Default::default(),
            current_channel: Default::default(),
            controls,
        }
    }

    pub fn from_iter(keys: impl IntoIterator<Item = FixtureGroupKey>) -> Self {
        let mut c = Self::new();
        for k in keys {
            c.add(k);
        }
        c
    }

    /// Add new channel controls, wired to the specified fixture.
    pub fn add(&mut self, group: FixtureGroupKey) -> ChannelId {
        let id = ChannelId(self.channel_index.len());
        self.channel_index.push(group.clone());
        self.fixture_channel_index.insert(group, id);
        // If this is the first channel we're configuring, set it as selected.
        if self.current_channel.is_none() {
            self.current_channel = Some(id);
        }
        id
    }

    /// Iterate over valid channel IDs.
    pub fn channel_ids(&self) -> impl Iterator<Item = ChannelId> + '_ {
        self.channel_index
            .iter()
            .enumerate()
            .map(|(i, _)| ChannelId(i))
    }

    /// Validate that a channel index refers to a channel that actually exists.
    pub fn validate_channel(&self, channel: usize) -> Result<ChannelId> {
        if channel < self.channel_index.len() {
            Ok(ChannelId(channel))
        } else {
            bail!(
                "channel selector {channel} out of range, only {} channels are configured",
                self.channel_index.len()
            );
        }
    }

    /// Look up a channel ID by fixture group key.
    pub fn channel_for_fixture(&self, group: &str) -> Option<ChannelId> {
        self.fixture_channel_index.get(group).cloned()
    }

    /// Iterate over all of the labels for each channels.
    pub fn channel_labels<'a>(&'a self, patch: &'a Patch) -> impl Iterator<Item = String> + 'a {
        self.channel_index
            .iter()
            .filter_map(|i| match patch.get(i) {
                Ok(f) => Some(f),
                Err(err) => {
                    error!("Patch inconsistency generating channel labels: {err}");
                    None
                }
            })
            .map(move |g| g.qualified_name().to_string())
    }

    /// Get a fixture group by channel ID.
    pub fn group_by_channel<'a>(
        &self,
        patch: &'a Patch,
        channel: ChannelId,
    ) -> Result<&'a FixtureGroup> {
        let Some(fixture_key) = self.channel_index.get(channel.0) else {
            bail!("tried to get out-of-range channel {channel}");
        };
        patch
            .get(fixture_key)
            .with_context(|| format!("channel {channel}"))
    }

    /// Get a fixture group by channel ID, mutably.
    pub fn group_by_channel_mut<'a>(
        &self,
        patch: &'a mut Patch,
        channel: ChannelId,
    ) -> Result<&'a mut FixtureGroup> {
        let Some(fixture_key) = self.channel_index.get(channel.0) else {
            bail!("tried to get out-of-range channel {channel}");
        };
        patch
            .get_mut(fixture_key)
            .with_context(|| format!("channel {channel}"))
    }

    pub fn current_channel(&self) -> Option<ChannelId> {
        self.current_channel
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
            StateChange::ChannelLabels(self.channel_labels(patch).collect()),
            &scoped_emitter,
        );
        if selected_fixture_only {
            if let Some(channel_id) = self.current_channel {
                match self.group_by_channel(patch, channel_id) {
                    Ok(f) => f.emit_state(ChannelStateEmitter {
                        channel_id: Some(channel_id),
                        emitter,
                    }),
                    Err(err) => error!("Failed to emit channel {channel_id} state: {err}."),
                }
            }
        } else {
            for channel_id in self.channel_ids() {
                match self.group_by_channel(patch, channel_id) {
                    Ok(f) => f.emit_state(ChannelStateEmitter {
                        channel_id: Some(channel_id),
                        emitter,
                    }),
                    Err(err) => error!("Failed to emit channel {channel_id} state: {err}."),
                }
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
                // Validate the channel.
                let channel = self.validate_channel(*g)?;
                if self.current_channel == Some(channel) {
                    // Channel is not changed, ignore.
                    return Ok(());
                }
                self.current_channel = Some(channel);
                self.emit_state(true, patch, emitter);
                // FIXME this is so goddamn inside out, I hate it.
                animation_ui.emit_state(
                    channel,
                    self.group_by_channel(patch, channel)?,
                    &ScopedControlEmitter {
                        entity: crate::osc::animation::GROUP,
                        emitter,
                    },
                );
            }
            ControlMessage::Control { channel_id, msg } => {
                let channel_id = if let Some(id) = channel_id {
                    self.validate_channel(*id)?
                } else {
                    self.current_channel.ok_or_else(|| {
                        anyhow!(
                            "no channel ID provided or selected for channel control message {msg:?}"
                        )
                    })?
                };
                let handled = self
                    .group_by_channel_mut(patch, channel_id)?
                    .control_from_channel(
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
