//! Control profile for WLED via http/json API.

use std::num::NonZeroU8;

use crate::{
    fixture::prelude::*,
    wled::{WledControlMessage, WledController},
};
use reqwest::Url;
use wled_json_api_library::structures::state::{Seg, State};

#[derive(Debug, EmitState, Control, DescribeControls, Update)]
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
    pub url: Url,
    pub preset_count: NonZeroU8,
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
            preset: IndexedSelect::new(
                "Preset",
                usize::from(options.preset_count.get()),
                false,
                (),
            ),
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
register_touchosc_template!(Wled);

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
        self.controller
            .send(WledControlMessage::SetState(self.preset_state()));
    }

    /// The state that loads the selected preset, with the current speed and size.
    /// WLED numbers presets from 1.
    fn preset_state(&self) -> State {
        let preset_id = self.preset.selected().saturating_add(1);
        let mut state = State {
            ps: Some(i32::try_from(preset_id).unwrap_or(i32::MAX)),
            ..Default::default()
        };
        // TODO: this may not actually do anything
        self.set_speed(&mut state);
        self.set_size(&mut state);
        state
    }
}

fn get_seg(state: &mut State) -> &mut Seg {
    let seg = state.seg.get_or_insert(vec![]);
    if seg.is_empty() {
        seg.push(Default::default());
    }
    &mut seg[0]
}

pub mod rug_doctor {
    //! Composite fixture, controlling both a WLED node and an Astera controller.
    use log::{debug, error};
    use serde::Serialize;

    use crate::color::ColorRgb;

    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct GroupOptions {
        #[serde(flatten)]
        pub wled: super::GroupOptions,
        pub presets: Vec<AsteraPreset>,
    }

    impl crate::fixture::patch::OptionsMenu for GroupOptions {
        fn menu() -> Vec<(String, crate::fixture::patch::PatchOption)> {
            super::GroupOptions::menu()
        }
    }

    #[derive(Debug, EmitState, Control, DescribeControls, Update)]
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
            if options.presets.len() != usize::from(options.wled.preset_count.get()) {
                error!(
                    "WLED has {} presets but only {} are defined for astera",
                    options.wled.preset_count,
                    options.presets.len(),
                );
            }
            Self {
                presets: options.presets,
                wled: Wled::new(options.wled),
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

    const FADE: u8 = 100;

    impl NonAnimatedFixture for RugDoctor {
        fn render(&self, _: &FixtureGroupControls, dmx_buf: &mut [u8]) {
            let preset_index = self.wled.preset.selected();
            let preset = self.presets.get(preset_index).unwrap_or_else(|| {
                debug!(
                    "selected WLED preset {preset_index} out of range for astera, using default"
                );
                &MISSING_PRESET
            });
            dmx_buf[0] = unipolar_to_range(
                0,
                255,
                UnipolarFloat::new(self.wled.level.control.val().val() * preset.level_scale),
            );
            dmx_buf[1] = 0; // strobe off
            dmx_buf[2] = preset.program_dmx_val;
            dmx_buf[3] = unipolar_to_range(
                0,
                255,
                UnipolarFloat::new(self.wled.speed.control.val().val() * preset.speed_scale),
            );
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

    #[derive(Debug, Serialize, Deserialize)]
    pub struct AsteraPreset {
        pub program_dmx_val: u8,
        pub level_scale: f64,
        pub speed_scale: f64,
        pub colors: [ColorRgb; 4],
    }

    // FIXME workaround
    impl AsPatchOption for Vec<AsteraPreset> {
        fn as_patch_option() -> crate::fixture::patch::PatchOption {
            crate::fixture::patch::PatchOption::Bool
        }
    }

    static MISSING_PRESET: AsteraPreset = AsteraPreset {
        program_dmx_val: 0,
        level_scale: 1.0,
        speed_scale: 1.0,
        colors: [[0, 0, 0]; 4],
    };
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::Options;
    use crate::control::mock::NoOpEmitter;

    fn options(preset_count: i64) -> Options {
        Options::from_entries([
            ("url".to_string(), "http://127.0.0.1:9".into()),
            ("preset_count".to_string(), preset_count.into()),
        ])
    }

    /// The Preset buttons load WLED presets 1 through preset_count, since WLED
    /// numbers presets from 1.
    #[test]
    fn preset_buttons_load_presets_from_one() {
        let Ok(group_options) = Wled::parse_group_options(options(6)) else {
            panic!("6 presets were rejected");
        };
        let mut wled = Wled::new(group_options);
        assert_eq!(wled.preset_state().ps, Some(1));
        wled.preset.control_direct(5, &NoOpEmitter).unwrap();
        assert_eq!(wled.preset_state().ps, Some(6));
    }

    #[test]
    fn preset_count_is_between_1_and_255() {
        assert!(Wled::parse_group_options(options(255)).is_ok());
        for count in [0, 256] {
            let Err(err) = Wled::parse_group_options(options(count)) else {
                panic!("{count} presets were accepted");
            };
            assert!(
                format!("{err:#}").contains(&format!(
                    "invalid value: integer `{count}`, expected a nonzero u8"
                )),
                "{err:#}"
            );
        }
    }
}
