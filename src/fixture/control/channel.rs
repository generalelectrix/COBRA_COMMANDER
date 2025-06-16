//! Control decorators to bind OSC controls to channel controls.

use std::marker::PhantomData;

use number::{BipolarFloat, Phase, UnipolarFloat};

use crate::{
    channel::{ChannelControlMessage, ChannelStateChange, KnobIndex, KnobValue},
    osc::{FixtureStateEmitter, OscControlMessage},
};

use super::{OscControl, RenderToDmxWithAnimations};

#[derive(Debug)]
pub struct ChannelControl<C, T, H>
where
    C: OscControl<T> + RenderToDmxWithAnimations,
    H: ChannelHandler<T>,
{
    pub control: C,
    #[allow(unused)]
    label: String,
    handler: H,
    /// If true, echo exact channel control messages.
    /// If false, use the handler's emit method to create the output message.
    /// This behavior can be used to control whether or not we want to echo
    /// the control update in the domain of the target control, or in the domain
    /// of the source of the control message. Examples where this is useful:
    ///
    /// - a level fader controlling a boolean shutter would want to echo the exact
    ///   channel control message, since we'd want virtual faders to track
    ///   each other and not snap to a boolean value
    /// - a hardware device may represent all knob inputs as unipolar, but these
    ///   could be connected to a bipolar control parameter, or vice versa.
    ///   In this case, we'd want to echo out the control value in the bipolar
    ///   domain, to things like indicator rings can be set correctly.
    exact_echo: bool,
    phantom: PhantomData<T>,
}

impl<C, T, H> ChannelControl<C, T, H>
where
    C: OscControl<T> + RenderToDmxWithAnimations,
    H: ChannelHandler<T>,
{
    pub fn wrap(control: C, label: String, exact_echo: bool, handler: H) -> Self {
        Self {
            control,
            label,
            exact_echo,
            handler,
            phantom: PhantomData,
        }
    }

    /// Control this channel-control-wrapped control from OSC.
    pub fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        self.control
            .control_with_callback(msg, emitter, |v| emitter.emit_channel(self.handler.emit(v)))
    }

    /// Emit state for this channel-wrapped control, including for the channel control.
    pub fn emit_state(&self, emitter: &FixtureStateEmitter) {
        self.control
            .emit_state_with_callback(emitter, |v| emitter.emit_channel(self.handler.emit(v)));
    }

    /// Handle a channel-level control.
    pub fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        let Some(v) = self.handler.parse(msg) else {
            return Ok(false);
        };
        emitter.emit_channel(if self.exact_echo {
            *msg
        } else {
            self.handler.emit(&v)
        });
        self.control.control_direct(v, emitter)?;
        Ok(true)
    }
}

/// Something that adapts channel control messages to a concrete type T.
pub trait ChannelHandler<T> {
    /// Parse a specific channel control message into a T.
    /// This will usually handle a specific channel control message.
    fn parse(&self, msg: &ChannelControlMessage) -> Option<T>;

    /// Handle converting T into a specific channel state change.
    fn emit(&self, val: &T) -> ChannelStateChange;
}

/// Channel level control.
#[derive(Debug)]
pub struct ChannelLevelHandler;

impl ChannelHandler<UnipolarFloat> for ChannelLevelHandler {
    fn parse(&self, msg: &ChannelControlMessage) -> Option<UnipolarFloat> {
        let ChannelControlMessage::Level(v) = msg else {
            return None;
        };
        Some(*v)
    }

    fn emit(&self, val: &UnipolarFloat) -> ChannelStateChange {
        ChannelStateChange::Level(*val)
    }
}

pub type ChannelLevelControl<C, T> = ChannelControl<C, T, ChannelLevelHandler>;

pub type ChannelLevelUnipolar<C> = ChannelLevelControl<C, UnipolarFloat>;

/// Channel level control for bool controls, like on/off shutters or non-dimmable lasers.
impl ChannelHandler<bool> for ChannelLevelHandler {
    fn parse(&self, msg: &ChannelControlMessage) -> Option<bool> {
        let ChannelControlMessage::Level(v) = msg else {
            return None;
        };
        Some(*v > 0.5)
    }

    fn emit(&self, val: &bool) -> ChannelStateChange {
        ChannelStateChange::Level(if *val {
            UnipolarFloat::ONE
        } else {
            UnipolarFloat::ZERO
        })
    }
}

pub type ChannelLevelBool<C> = ChannelLevelControl<C, bool>;

/// Delegate rendering to the inner control.
impl<C, T, H> RenderToDmxWithAnimations for ChannelControl<C, T, H>
where
    C: OscControl<T> + RenderToDmxWithAnimations,
    H: ChannelHandler<T>,
{
    fn render(&self, animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        self.control.render(animations, dmx_buf);
    }
    fn render_no_anim(&self, dmx_buf: &mut [u8]) {
        self.control.render_no_anim(dmx_buf);
    }
    fn render_with_group(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
        self.control
            .render_with_group(group_controls, animations, dmx_buf);
    }
}

/// Channel knob control.
#[derive(Debug)]
pub struct ChannelKnobHandler {
    pub index: KnobIndex,
}

impl ChannelKnobHandler {
    /// Return the knob value if this channel control message is for this handler.
    fn matches<'a>(&self, msg: &'a ChannelControlMessage) -> Option<&'a KnobValue> {
        let ChannelControlMessage::Knob { index, value } = msg else {
            return None;
        };
        if *index != self.index {
            return None;
        }
        Some(value)
    }
}

impl ChannelHandler<UnipolarFloat> for ChannelKnobHandler {
    fn parse(&self, msg: &ChannelControlMessage) -> Option<UnipolarFloat> {
        Some(self.matches(msg)?.as_unipolar())
    }

    fn emit(&self, val: &UnipolarFloat) -> ChannelStateChange {
        ChannelStateChange::Knob {
            index: self.index,
            value: KnobValue::Unipolar(*val),
        }
    }
}

impl ChannelHandler<Phase> for ChannelKnobHandler {
    fn parse(&self, msg: &ChannelControlMessage) -> Option<Phase> {
        Some(self.matches(msg)?.as_unipolar().as_phase())
    }

    fn emit(&self, val: &Phase) -> ChannelStateChange {
        ChannelStateChange::Knob {
            index: self.index,
            value: KnobValue::Unipolar(val.as_unipolar()),
        }
    }
}

impl ChannelHandler<BipolarFloat> for ChannelKnobHandler {
    fn parse(&self, msg: &ChannelControlMessage) -> Option<BipolarFloat> {
        Some(self.matches(msg)?.as_bipolar())
    }

    fn emit(&self, val: &BipolarFloat) -> ChannelStateChange {
        ChannelStateChange::Knob {
            index: self.index,
            value: KnobValue::Bipolar(*val),
        }
    }
}

pub type ChannelKnobControl<C, T> = ChannelControl<C, T, ChannelKnobHandler>;

pub type ChannelKnobUnipolar<C> = ChannelKnobControl<C, UnipolarFloat>;

pub type ChannelKnobPhase<C> = ChannelKnobControl<C, Phase>;

pub type ChannelKnobBipolar<C> = ChannelKnobControl<C, BipolarFloat>;
