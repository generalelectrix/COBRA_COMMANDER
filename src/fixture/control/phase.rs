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

impl<R: RenderToDmx<Phase>> super::DescribeOscControls for PhaseControl<R> {
    fn describe_controls(&self) -> Vec<super::OscControlDescription> {
        vec![super::OscControlDescription {
            name: self.name.clone(),
            control_type: super::OscControlType::Phase,
        }]
    }
}

impl<R: RenderToDmx<Phase>> RenderToDmxWithAnimations for PhaseControl<R> {
    fn render(
        &self,
        _group_controls: &crate::fixture::FixtureGroupControls,
        animations: impl Iterator<Item = f64>,
        dmx_buf: &mut [u8],
    ) {
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

#[cfg(test)]
mod tests {
    use number::Phase;
    use rosc::{OscMessage, OscType};

    use crate::osc::{MockEmitter, OscClientId, OscControlMessage};

    use super::*;

    fn make_msg(addr: &str, arg: OscType) -> OscControlMessage {
        OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    #[test]
    fn test_new_defaults_to_zero() {
        let ctrl = PhaseControl::new("X", ());
        assert_eq!(ctrl.val(), Phase::ZERO);
    }

    #[test]
    fn test_at_half_sets_half() {
        let ctrl = PhaseControl::new("X", ()).at_half();
        assert!((ctrl.val().val() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_control_matching_name() {
        let mut ctrl = PhaseControl::new("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/X", OscType::Float(0.75));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(handled);
        assert!((ctrl.val().val() - 0.75).abs() < 1e-6);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "X");
    }

    #[test]
    fn test_control_non_matching() {
        let mut ctrl = PhaseControl::new("X", ());
        let emitter = MockEmitter::new();
        let msg = make_msg("/g/Y", OscType::Float(0.75));
        let handled = ctrl.control(&msg, &emitter).unwrap();
        assert!(!handled);
    }

    #[test]
    fn test_val_with_anim() {
        let ctrl = PhaseControl::new("X", ()).at_half();
        // Phase wraps, so 0.5 + 0.2 should be ~0.7 (no wrap needed)
        let result = ctrl.val_with_anim([0.2].into_iter());
        assert!((result.val() - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_render_phase_to_range() {
        let render = RenderPhaseToRange {
            dmx_buf_offset: 0,
            start: 0,
            end: 255,
        };
        let mut buf = [0u8; 1];
        render.render(&Phase::new(0.5), &mut buf);
        assert_eq!(buf[0], 127);
    }
}
