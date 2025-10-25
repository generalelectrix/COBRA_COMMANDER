//! Control profile for WLED via http/json API.

use crate::{
    fixture::prelude::*,
    wled::{WledControlMessage, WledController},
};
use reqwest::Url;
use wled_json_api_library::structures::state::{Seg, State};

#[derive(Debug, EmitState, Control, Update)]
pub struct Wled {
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
    #[skip_control]
    #[skip_emit]
    controller: WledController,
}

const URL_OPT: &str = "url";
const PRESET_COUNT_OPT: &str = "preset_count";

#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct GroupOptions {
    url: Url,
    preset_count: usize,
}

impl PatchFixture for Wled {
    const NAME: FixtureType = FixtureType("Wled");
    type GroupOptions = GroupOptions;

    fn new(options: Self::GroupOptions) -> Result<Self> {
        Ok(Self {
            level: Unipolar::new("Level", ()).with_channel_level(),
            speed: Unipolar::new("Speed", ()).with_channel_knob(0),
            size: Unipolar::new("Size", ()).with_channel_knob(1),
            preset: IndexedSelect::new("Preset", options.preset_count, false, ()),
            controller: WledController::run(options.url),
        })
    }

    fn group_options() -> Vec<(String, PatchOption)> {
        vec![
            (URL_OPT.to_string(), PatchOption::Url),
            (PRESET_COUNT_OPT.to_string(), PatchOption::Int),
        ]
    }

    fn patch_options() -> Vec<(String, PatchOption)> {
        vec![]
    }
}

impl CreatePatchConfig for Wled {
    fn patch(&self, options: Options) -> Result<PatchConfig> {
        options.ensure_empty()?;
        Ok(PatchConfig {
            channel_count: 0,
            render_mode: None,
        })
    }
}

register_patcher!(Wled);

impl NonAnimatedFixture for Wled {
    fn render(&self, _: &FixtureGroupControls, _: &mut [u8]) {}
}

impl Wled {
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

    fn update_level(&self, _emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        self.controller.send(WledControlMessage::SetState(state));
    }

    fn update_speed(&self, _emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        self.controller.send(WledControlMessage::SetState(state));
    }

    fn update_effect_intensity(&self, _emitter: &FixtureStateEmitter) {
        let mut state = State::default();
        self.set_level(&mut state);
        self.set_speed(&mut state);
        self.set_size(&mut state);
        self.controller.send(WledControlMessage::SetState(state));
    }

    fn update_preset(&self, _emitter: &FixtureStateEmitter) {
        let mut state = State {
            ps: Some(self.preset.selected() as i32),
            ..Default::default()
        };
        // TODO: this may not actually do anything
        self.set_speed(&mut state);
        self.set_size(&mut state);
        self.controller.send(WledControlMessage::SetState(state));
    }
}

fn get_seg(state: &mut State) -> &mut Seg {
    let seg = state.seg.get_or_insert(vec![]);
    if seg.is_empty() {
        seg.push(Default::default());
    }
    &mut seg[0]
}
