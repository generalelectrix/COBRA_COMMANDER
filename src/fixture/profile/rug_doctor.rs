//! Control profile for the Rug Doctor WLED interface.
use crate::{
    fixture::prelude::*,
    wled::{EmitWledControlMessage, WledControlMessage},
};
use wled_json_api_library::structures::state::{Seg, State};

#[derive(Debug, EmitState, Control, PatchFixture)]
#[channel_count = 0]
pub struct RugDoctor {
    #[channel_control]
    #[on_change = "update_level"]
    level: ChannelLevelUnipolar<Unipolar<()>>,
    #[channel_control]
    #[on_change = "update_speed"]
    speed: ChannelKnobUnipolar<Unipolar<()>>,
    #[channel_control]
    #[on_change = "update_effect_intensity"]
    size: ChannelKnobUnipolar<Unipolar<()>>,
    #[on_change = "update_preset"]
    preset: IndexedSelect<()>,
}

impl Default for RugDoctor {
    fn default() -> Self {
        Self {
            level: Unipolar::new("Level", ()).with_channel_level(),
            speed: Unipolar::new("Speed", ()).with_channel_knob(0),
            size: Unipolar::new("Size", ()).with_channel_knob(1),
            preset: IndexedSelect::new("Preset", 6, false, ()),
        }
    }
}

impl NonAnimatedFixture for RugDoctor {
    fn render(&self, _: &FixtureGroupControls, _: &mut [u8]) {}
}

impl ControllableFixture for RugDoctor {}

impl RugDoctor {
    fn set_level(&self, state: &mut State) {
        let level = unipolar_to_range(0, 255, self.level.control.val());
        if level == 0 {
            state.on = Some(false);
        } else {
            state.on = Some(true);
            state.bri = Some(level);
        };
    }

    fn set_speed(&self, state: &mut State) {
        get_seg(state).sx = Some(unipolar_to_range(0, 255, self.speed.control.val()));
    }

    fn set_size(&self, state: &mut State) {
        get_seg(state).ix = Some(unipolar_to_range(0, 255, self.size.control.val()))
    }

    fn update_level(&self, emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        emitter.emit_wled(WledControlMessage::SetState(state));
    }

    fn update_speed(&self, emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        emitter.emit_wled(WledControlMessage::SetState(state));
    }

    fn update_effect_intensity(&self, emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        emitter.emit_wled(WledControlMessage::SetState(state));
    }

    fn update_preset(&self, emitter: &FixtureStateEmitter) {
        let mut state = State {
            ps: Some(self.preset.selected() as i32),
            ..Default::default()
        };
        // TODO: this may not actually do anything
        self.set_speed(&mut state);
        self.set_size(&mut state);
        emitter.emit_wled(WledControlMessage::SetState(state));
    }
}

fn get_seg(state: &mut State) -> &mut Seg {
    let seg = state.seg.get_or_insert(vec![]);
    if seg.is_empty() {
        seg.push(Default::default());
    }
    &mut seg[0]
}
