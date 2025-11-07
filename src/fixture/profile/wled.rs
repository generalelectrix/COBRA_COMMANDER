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

#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct GroupOptions {
    url: Url,
    preset_count: usize,
}

impl PatchFixture for Wled {
    const NAME: FixtureType = FixtureType("Wled");
    type GroupOptions = GroupOptions;
    type PatchOptions = NoOptions;

    fn new(options: Self::GroupOptions) -> Self {
        Self {
            level: Unipolar::new("Level", ()).with_channel_level(),
            speed: Unipolar::new("Speed", ()).with_channel_knob(0),
            size: Unipolar::new("Size", ()).with_channel_knob(1),
            preset: IndexedSelect::new("Preset", options.preset_count, false, ()),
            controller: WledController::run(options.url),
        }
    }

    fn new_patch(_: Self::GroupOptions, _: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: 0,
            render_mode: None,
        }
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

mod rug_doctor {
    //! Composite fixture, controlling both a WLED node and an Astera controller.
    use log::error;

    use crate::color::ColorRgb;

    use super::*;

    #[derive(Debug, EmitState, Control, Update)]
    pub struct RugDoctor {
        #[channel_control]
        wled: Wled,

        #[skip_emit]
        #[skip_control]
        presets: Vec<AsteraPreset>,
    }

    impl PatchFixture for RugDoctor {
        const NAME: FixtureType = FixtureType("RugDoctor");
        type GroupOptions = GroupOptions;
        type PatchOptions = NoOptions;

        fn new(options: Self::GroupOptions) -> Self {
            Self {
                presets: get_presets(options.preset_count),
                wled: Wled::new(options),
            }
        }

        fn new_patch(_: Self::GroupOptions, _: Self::PatchOptions) -> PatchConfig {
            PatchConfig {
                channel_count: 20,
                render_mode: None,
            }
        }
    }

    register_patcher!(RugDoctor);

    const LEVEL_SCALE: UnipolarFloat = UnipolarFloat::ONE;
    const SPEED_SCALE: UnipolarFloat = UnipolarFloat::ONE;
    const FADE: u8 = 100;

    impl NonAnimatedFixture for RugDoctor {
        fn render(&self, _: &FixtureGroupControls, dmx_buf: &mut [u8]) {
            let preset_index = self.wled.preset.selected();
            let preset = self.presets.get(preset_index).unwrap_or_else(|| {
                error!(
                    "selected WLED preset {preset_index} out of range for astera, using default"
                );
                &DEFAULT_PRESET
            });
            dmx_buf[0] = unipolar_to_range(0, 255, self.wled.level.control.val() * LEVEL_SCALE);
            dmx_buf[1] = 0; // strobe off
            dmx_buf[2] = preset.program_dmx_val;
            dmx_buf[3] = unipolar_to_range(0, 255, self.wled.speed.control.val() * SPEED_SCALE);
            dmx_buf[4] = FADE;
            dmx_buf[5] = 0; // pattern forward, pattern loops
            dmx_buf[6] = 0;
            dmx_buf[7] = 0; // send on modify
            // write palette
            for (i, color) in preset.colors.iter().enumerate() {
                let start = 8 + i * 3;
                dmx_buf[start] = color[0];
                dmx_buf[start + 1] = color[1];
                dmx_buf[start + 2] = color[2];
            }
        }
    }

    #[derive(Debug, Default)]
    struct AsteraPreset {
        program_dmx_val: u8,
        colors: [ColorRgb; 4],
    }

    static DEFAULT_PRESET: AsteraPreset = AsteraPreset {
        program_dmx_val: 0,
        colors: [[0, 0, 0]; 4],
    };

    fn get_presets(count: usize) -> Vec<AsteraPreset> {
        let presets = vec![];
        if presets.len() < count {
            panic!(
                "WLED has {count} presets but only {} are defined for astera",
                presets.len()
            );
        }
        presets
    }
}
