//! A control for phases.

use anyhow::Context;
use number::{Phase, UnipolarFloat};

use crate::{
    channel::KnobIndex,
    osc::{EmitScopedOscMessage, OscControlMessage},
    util::unipolar_to_range,
};

use super::{
    ChannelControl, ChannelKnobHandler, ChannelKnobPhase, OscControl, RenderToDmx,
    RenderToDmxWithAnimations,
};

/// A phase value, with controls.
#[derive(Debug)]
pub struct PhaseControl<R: RenderToDmx<Phase>> {
    val: Phase,
    name: String,
    render: R,
}

/// A phase control that renders into a single DMX channel over a range.
#[allow(unused)]
pub type PhaseChannel = PhaseControl<RenderPhaseToRange>;

impl<R: RenderToDmx<Phase>> PhaseControl<R> {
    /// Initialize a new control with the provided OSC control name.
    pub fn new<S: Into<String>>(name: S, render: R) -> Self {
        Self {
            val: Phase::ZERO,
            name: name.into(),
            render,
        }
    }

    pub fn val(&self) -> Phase {
        self.val
    }

    pub fn val_with_anim(&self, animations: impl Iterator<Item = f64>) -> Phase {
        let mut val = self.val.val();
        for anim_val in animations {
            // TODO: configurable blend modes
            val += anim_val;
        }
        Phase::new(val)
    }

    /// Set the initial value of this control to 0.5.
    pub fn at_half(mut self) -> Self {
        self.val = Phase::new(0.5);
        self
    }

    /// Decorate this control with a channel knob of the provided index.
    pub fn with_channel_knob(self, index: KnobIndex) -> ChannelKnobPhase<Self> {
        let label = self.name.clone();
        ChannelControl::wrap(self, label, false, ChannelKnobHandler { index })
    }
}

impl PhaseControl<RenderPhaseToRange> {
    /// Initialize a phase control that renders to a full DMX channel.
    #[allow(unused)]
    pub fn full_channel<S: Into<String>>(name: S, dmx_buf_offset: usize) -> Self {
        Self::new(
            name,
            RenderPhaseToRange {
                dmx_buf_offset,
                start: 0,
                end: 255,
            },
        )
    }
}

impl<R: RenderToDmx<Phase>> OscControl<Phase> for PhaseControl<R> {
    fn control_direct(
        &mut self,
        val: Phase,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        self.val = val;
        emitter.emit_float(&self.name, self.val.into());
        Ok(())
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if msg.control() != self.name {
            return Ok(false);
        }
        self.control_direct(msg.get_phase().with_context(|| self.name.clone())?, emitter)?;
        Ok(true)
    }

    fn control_with_callback(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&Phase),
    ) -> anyhow::Result<bool> {
        if self.control(msg, emitter)? {
            callback(&self.val);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn emit_state(&self, emitter: &dyn EmitScopedOscMessage) {
        emitter.emit_float(&self.name, self.val.into());
    }

    fn emit_state_with_callback(
        &self,
        emitter: &dyn EmitScopedOscMessage,
        callback: impl Fn(&Phase),
    ) {
        self.emit_state(emitter);
        callback(&self.val);
    }
}

impl<R: RenderToDmx<Phase>> RenderToDmxWithAnimations for PhaseControl<R> {
    fn render(&self, animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        self.render.render(&self.val_with_anim(animations), dmx_buf);
    }
}

/// Render a phase float to a continuous range.
#[derive(Debug)]
pub struct RenderPhaseToRange {
    pub dmx_buf_offset: usize,
    pub start: u8,
    pub end: u8,
}

impl RenderToDmx<Phase> for RenderPhaseToRange {
    fn render(&self, val: &Phase, dmx_buf: &mut [u8]) {
        dmx_buf[self.dmx_buf_offset] =
            unipolar_to_range(self.start, self.end, UnipolarFloat::new(val.val()));
    }
}
