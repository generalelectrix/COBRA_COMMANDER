//! A DMX faderboard utility.

use log::error;

use crate::fixture::prelude::*;

#[derive(Debug, Update, PatchFixture)]
#[channel_count = 16]
pub struct Faderboard {
    controls: GroupControlMap<ControlMessage>,
    vals: Vec<UnipolarFloat>,
}

const CHANNEL_COUNT: usize = 16;

impl Default for Faderboard {
    fn default() -> Self {
        let mut controls = GroupControlMap::default();
        CONTROLS.map(&mut controls, |index, val| Ok((index, val)));
        Self {
            controls,
            vals: vec![UnipolarFloat::ZERO; CHANNEL_COUNT],
        }
    }
}

impl Faderboard {
    fn handle_state_change(&mut self, sc: StateChange, emitter: &FixtureStateEmitter) {
        let (chan, val) = sc;
        if chan >= CHANNEL_COUNT {
            error!("Channel out of range: {chan}.");
            return;
        }
        self.vals[chan] = val;
        Self::emit(sc, emitter);
    }

    fn emit(_sc: StateChange, _emitter: &FixtureStateEmitter) {
        // FIXME: no talkback
    }
}

impl NonAnimatedFixture for Faderboard {
    fn render(&self, _group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        for (i, v) in self.vals.iter().enumerate() {
            dmx_buf[i] = unipolar_to_range(0, 255, *v);
        }
    }
}

impl crate::fixture::EmitState for Faderboard {
    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        for (i, v) in self.vals.iter().enumerate() {
            Self::emit((i, *v), emitter);
        }
    }
}

impl crate::fixture::Control for Faderboard {
    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        let Some((ctl, _)) = self.controls.handle(msg)? else {
            return Ok(true);
        };
        self.handle_state_change(ctl, emitter);
        Ok(true)
    }

    fn control_from_channel(
        &mut self,
        _msg: &crate::channel::ChannelControlMessage,
        _emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

pub type StateChange = (usize, UnipolarFloat);

pub type ControlMessage = StateChange;

const CONTROLS: UnipolarArray = unipolar_array("Fader");
