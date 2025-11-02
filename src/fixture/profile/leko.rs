//! Control a single leko with a gobo rotator, most flexibly a DHA Varispeed.
//!
//! NOTE: we do not have a clean way to specify that a single fixture requires
//! more than one distinct DMX buffer range to render into. We currently hack
//! our way around this by patching two instances of Leko per fixture, one with
//! the dimmer model, and the other with the rotator model. This essentially
//! works, though it does imply that the animation phase of the dimmers will be
//! slightly offset from the animation phase of the rotators.  This *probably*
//! doesn't matter in practice, and it will especially not matter if we're using
//! random noise generation to drive them.

use log::error;
use ordered_float::OrderedFloat;
use strum_macros::{Display, EnumIter, EnumString, VariantArray};

use crate::fixture::{patch::NoOptions, prelude::*};

#[derive(Debug, EmitState, Control, Update)]
pub struct Leko {
    #[channel_control]
    #[animate]
    level: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    gobo1: ChannelKnobBipolar<Bipolar<()>>,
    #[channel_control]
    #[animate]
    gobo2: ChannelKnobBipolar<Bipolar<()>>,

    #[skip_control]
    #[skip_emit]
    roto_q_lut: SpeedLookupTable,

    #[skip_control]
    #[skip_emit]
    smart_move_lut: SpeedLookupTable,
}

impl Default for Leko {
    fn default() -> Self {
        Self {
            level: Unipolar::full_channel("Level", 0)
                .strobed()
                .with_channel_level(),
            gobo1: Bipolar::new("Gobo1", ()).with_detent().with_channel_knob(0),
            gobo2: Bipolar::new("Gobo2", ()).with_detent().with_channel_knob(1),
            roto_q_lut: roto_q_lut(),
            smart_move_lut: smart_move_lut(),
        }
    }
}

impl PatchFixture for Leko {
    const NAME: FixtureType = FixtureType("Leko");
    type GroupOptions = NoOptions;
    type PatchOptions = PatchOptions;

    fn new(_options: Self::GroupOptions) -> Self {
        Default::default()
    }

    fn can_strobe() -> Option<StrobeResponse> {
        Some(StrobeResponse::Long)
    }

    fn new_patch(_: Self::GroupOptions, options: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: options.kind.channel_count(),
            render_mode: Some(options.kind.render_mode()),
        }
    }
}

#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct PatchOptions {
    kind: Model,
}

register_patcher!(Leko);

impl AnimatedFixture for Leko {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let model = match Model::model_for_mode(group_controls.render_mode) {
            Ok(m) => m,
            Err(err) => {
                error!("failed to render Leko: {err}");
                return;
            }
        };
        match model {
            Model::Dimmer => {
                self.level.render(
                    group_controls,
                    animation_vals.filter(&AnimationTarget::Level),
                    dmx_buf,
                );
            }
            Model::GoboSpinnaz => {
                render_gobo_spinna(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    &mut dmx_buf[0..2],
                );
                render_gobo_spinna(
                    self.gobo2
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo2)),
                    &mut dmx_buf[2..4],
                );
            }
            Model::DhaVarispeed => {
                render_varispeed(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    &mut dmx_buf[0..2],
                );
                render_varispeed(
                    self.gobo2
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo2)),
                    &mut dmx_buf[2..4],
                );
            }
            Model::DcRotator => {
                let animated = self
                    .gobo1
                    .control
                    .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1));
                dmx_buf[0] = unipolar_to_range(0, 255, animated.abs());
            }
            Model::ApolloRotoQDmx => {
                render_roto_q(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    dmx_buf,
                    &self.roto_q_lut,
                );
            }
            Model::ApolloSmartMove => {
                render_smart_move(
                    self.gobo1
                        .control
                        .val_with_anim(animation_vals.filter(&AnimationTarget::Gobo1)),
                    dmx_buf,
                    &self.smart_move_lut,
                );
            }
        }
    }
}

/// Which model of gobo rotator is installed in this leko, or is this the dimmer.
#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    Eq,
    PartialEq,
    EnumString,
    Deserialize,
    VariantArray,
    Display,
    EnumIter,
)]
enum Model {
    /// Dimmer channel.
    #[default]
    Dimmer,
    /// DHA Varispeed controlled using the GOBO SPINNAZ module.
    GoboSpinnaz,
    /// DHA Varispeed controlled using the DHA DMX DC controller.
    DhaVarispeed,
    /// Dimmer channel controlling a DC wall wart, such as TwinSpin OG or film FX.
    DcRotator,
    /// Apollo Roto-Q DMX rotator.
    ApolloRotoQDmx,
    /// Apollo Smart Move DMX rotator.
    ApolloSmartMove,
}

impl EnumRenderModel for Model {}

impl Model {
    fn channel_count(&self) -> usize {
        match self {
            Self::Dimmer => 1,
            Self::GoboSpinnaz => 4,
            Self::DhaVarispeed => 4,
            Self::DcRotator => 1,
            Self::ApolloRotoQDmx => 2,
            Self::ApolloSmartMove => 3,
        }
    }
}

/*
--- GOBO SPINNAZ ---

(DHA Varispeed driven by GOBO SPINNAZ driver)

Unsurprisingly, straight as an arrow, given linear voltage drive of a DC motor.
Max speed is MUCH slower than the Apollo rotators.

[
    (15, 0.0075),
    (35, 0.0225),
    (55, 0.0377),
    (75, 0.0523),
    (95, 0.0669),
    (115, 0.0825),
    (135, 0.0963),
    (155, 0.111),
    (175, 0.127),
    (195, 0.141),
    (215, 0.156),
    (235, 0.17),
    (255, 0.185),
]

If we normalize speed so the fastest rotator at max is 1, we lose a lot of the
upper range and resolution of the faster rotators.  I think I'll scale them so
that the slowest rotator's max speed is 1.0, but make the profiles understand
control signals outside of 1.0 if we want to reach up to higher values.

1.0 thus means 0.185 Hz or 11.1 rpm.
*/

const UNIT_SPEED: f64 = 0.185; // Hz

/// Control profile for custom DHA Varispeed driven by GOBO SPINNAZ.
///
/// Channel layout:
/// 0: gobo 1 direction
/// 1: gobo 1 speed
/// 2: gobo 2 direction
/// 3 gobo 2 speed
fn render_gobo_spinna(val: BipolarFloat, dmx_buf: &mut [u8]) {
    // direction
    dmx_buf[0] = if val <= BipolarFloat::ZERO { 0 } else { 255 };
    // speed
    dmx_buf[1] = unipolar_to_range(0, 255, val.abs());
}

/*
/// DHA Varispeed measurements

[
    (5, 0),
    (10, 0.005331),
    (15, 0.009365),
    (20, 0.012991),
    (25, 0.016860),
    (30, 0.020665888353944173),
    (35, 0.02436561608726548),
    (40, 0.028177784904943316),
    (45, 0.03203382037453513),
    (50, 0.03575878517267219),
    (55, 0.039799290612457676),
    (60, 0.0432223698484465),
    (100, 0.07435610885207719),
    (150, 0.11187585336998046),
    (200, 0.14974241458862855),
    (250, 0.1871409362476808),
    (255, 0.19027611195544866),
]
*/

/// DHA Varispeed driven by DHA DC Controller DMX.
///
/// Unsurprisingly, speed ramp is nearly identical to the GOBO SPINNAZ, but with
/// a small detent near DMX 0.
fn render_varispeed(val: BipolarFloat, dmx_buf: &mut [u8]) {
    // varispeed at DMX 250 is very close to GOBO SPINNA at DMX 255,
    // varispeed has a small detent such that DMX value 5 isn't spinning at all.
    let mut speed_int = (val.val().abs() * 245.) as u8;
    if speed_int > 0 {
        // If we're spinning at all, start from DMX value 6, and range up to 250.
        speed_int += 5;
    }
    if val < BipolarFloat::ZERO {
        dmx_buf[0] = speed_int;
        dmx_buf[1] = 0;
    } else {
        dmx_buf[0] = 0;
        dmx_buf[1] = speed_int;
    }
}

/*
--- Roto-Q DMX ---
0: stopped

Max speed: DMX value 1, about 0.43 rot/sec
It looks like several values are bucketed to the same speed:
3 4 5
6 7
8 9 10
11 12
14 15
16 17
19 20

255 254
252 251 250
249 248
247 246 245
244 243
241 240
239 238
236 235

These are all above unit speed, so fine to ignore them.

There's no actual DMX value in the center for no rotation. 127 and 128 are each
the slowest value for each rotation direction.  This explains some things.
*/

/// Control profile for Apollo Roto-Q DMX.
///
/// Channel layout:
/// 0: direction/speed
/// 1: set to 0 for rotation mode
fn render_roto_q(speed: BipolarFloat, dmx_buf: &mut [u8], lookup_table: &SpeedLookupTable) {
    // The negation on the value is to make direction consistent with the other two rotators.
    let speed = speed.invert();
    dmx_buf[0] = lookup_table.dmx_val_for_speed(speed);
    dmx_buf[1] = 0;
}

const ROTO_Q_MEAS: [(u8, f64); 15] = [
    (128, 0.00479),
    (137, 0.01),
    (147, 0.0169),
    (157, 0.03),
    (167, 0.0454),
    (177, 0.063),
    (187, 0.0792),
    (197, 0.106),
    (207, 0.1425),
    (217, 0.177),
    (227, 0.242),
    (237, 0.308),
    (242, 0.345),
    (249, 0.3875),
    (255, 0.43),
];

/// Construct the speed to DMX look-up table for the Roto-Q DMX.
fn roto_q_lut() -> SpeedLookupTable {
    let lut = build_lut(&ROTO_Q_MEAS); // upper range LUT

    // Prepare reverse LUT (lower range)
    let first_section = &lut[..lut.len() - 5];
    let second_section: Vec<(f64, u8)> = lut[lut.len() - 4..]
        .iter()
        .map(|&(s, v)| (s, v - 1))
        .collect();

    let reverse_lut: Vec<(f64, i16)> = first_section
        .iter()
        .chain(second_section.iter())
        .map(|&(s, v)| (-s, -(v as i16)))
        .rev()
        .collect();

    assert_eq!(reverse_lut.len(), lut.len() - 1);

    let mut speeds: Vec<f64> = Vec::new();
    let mut dmx_vals: Vec<u8> = Vec::new();

    // Lower half: shifted up from 127 using signed offset, clamped to u8
    for (s, v) in reverse_lut {
        let dmx = 127i16 + v;
        assert!((0..=255).contains(&dmx), "DMX value out of range");
        speeds.push(s);
        dmx_vals.push(dmx as u8);
    }

    // Center detent
    speeds.push(0.0);
    dmx_vals.push(0);

    // Upper half: offset from 128, safe since v is u8 and small
    for (s, v) in lut {
        let dmx = 128u8 + v;
        speeds.push(s);
        dmx_vals.push(dmx);
    }

    let speeds: Vec<_> = speeds.into_iter().map(OrderedFloat).collect();
    assert!(speeds.is_sorted());

    SpeedLookupTable { speeds, dmx_vals }
}

/*
--- Smart Move DMX ---

Bucketed speeds:
5 6 7 8
18 19
251 250 249 248
238 237

Slowest: 124, 133
Stopped: 125-132

The LUT profile below seems to run a bit faster than expected compared to the
other two profiles.  Should shake this out in the future.  For now, close enough
for rave work.
*/

/// Control profile for Apollo Smart Move DMX.
//
// Channel layout:
// 0: direction/speed
// 1: set to 0 for rotation mode
// 2: set to 0 for rotation mode
fn render_smart_move(speed: BipolarFloat, dmx_buf: &mut [u8], lookup_table: &SpeedLookupTable) {
    dmx_buf[0] = lookup_table.dmx_val_for_speed(speed);
    dmx_buf[1] = 0;
    dmx_buf[2] = 0;
}

// This shit is bananas, super-weird speed profile.
const SMART_MOVE_MEAS: [(u8, f64); 24] = [
    (133, 0.001776), // this point wasn't actually measured, just extrapolated down
    (135, 0.00193),
    (145, 0.0027),
    (155, 0.00583),
    (165, 0.0102),
    (175, 0.0175),
    (176, 0.0194),
    (179, 0.0244),
    (181, 0.0306),
    (183, 0.0406),
    (184, 0.0481),
    (185, 0.0604),
    (187, 0.0794),
    (189, 0.0909),
    (191, 0.0972),
    (193, 0.107),
    (195, 0.116),
    (205, 0.141),
    (215, 0.166),
    (225, 0.191),
    (235, 0.223),
    (245, 0.27),
    (249, 0.293),
    (255, 0.344),
];

/// Construct the speed to DMX look-up table for the Smart Move DMX.
fn smart_move_lut() -> SpeedLookupTable {
    let lut = build_lut(&SMART_MOVE_MEAS); // upper range LUT

    // Prepare reverse LUT (lower range)
    let reverse_lut: Vec<(f64, i16)> = lut.iter().map(|&(s, v)| (-s, -(v as i16))).rev().collect();

    let mut speeds: Vec<f64> = Vec::new();
    let mut dmx_vals: Vec<u8> = Vec::new();

    // Lower half: shifted up from 124
    for (s, v) in reverse_lut {
        let dmx = 124i16 + v;
        assert!((0..=255).contains(&dmx), "DMX value out of range");
        speeds.push(s);
        dmx_vals.push(dmx as u8);
    }

    // Center detent
    speeds.push(0.0);
    dmx_vals.push(130);

    // Upper half: offset from 133
    for (s, v) in lut {
        let dmx = 133u8 + v;
        speeds.push(s);
        dmx_vals.push(dmx);
    }

    let speeds: Vec<_> = speeds.into_iter().map(OrderedFloat).collect();
    assert!(speeds.is_sorted());

    SpeedLookupTable { speeds, dmx_vals }
}

/// Build a reverse speed lookup table from rotator measurements.
///
/// We use linear interpolation between measured points.
fn build_lut(meas: &[(u8, f64)]) -> Vec<(f64, u8)> {
    fn delta<T>(values: &[T]) -> Vec<T>
    where
        T: Copy + std::ops::Sub<Output = T>,
    {
        values.windows(2).map(|w| w[1] - w[0]).collect()
    }

    let v: Vec<_> = meas.iter().map(|m| m.0).collect();
    let s: Vec<_> = meas.iter().map(|m| m.1 / UNIT_SPEED).collect();

    let dv: Vec<_> = delta(&v);
    let ds: Vec<_> = delta(&s);
    let ds_dv: Vec<_> = std::iter::zip(ds, dv)
        .map(|(ds, dv)| ds / dv as f64)
        .collect();

    let min_value = v[0];

    (v[0]..=*v.last().unwrap())
        .map(|value| {
            let mut base_s = s[0];
            let mut base_v = v[0];
            let mut base_ds_dv = ds_dv[0];

            for ((&v0, &s0), &ds0) in v.iter().zip(s.iter()).zip(ds_dv.iter()) {
                if value < v0 {
                    break;
                }
                base_s = s0;
                base_v = v0;
                base_ds_dv = ds0;
            }

            let dv = value - base_v;
            let speed = base_s + (dv as f64) * base_ds_dv;
            (speed, value - min_value)
        })
        .collect()
}
#[derive(Debug)]
struct SpeedLookupTable {
    speeds: Vec<OrderedFloat<f64>>,
    dmx_vals: Vec<u8>,
}

impl SpeedLookupTable {
    fn dmx_val_for_speed(&self, val: BipolarFloat) -> u8 {
        let speed = OrderedFloat(val.val());
        match self.speeds.binary_search(&speed) {
            Ok(i) => self.dmx_vals[i],
            Err(i) => {
                if i == 0 {
                    self.dmx_vals[0]
                } else if i == self.dmx_vals.len() {
                    self.dmx_vals[i - 1]
                } else if self.speeds[i] - speed < speed - self.speeds[i - 1] {
                    self.dmx_vals[i]
                } else {
                    self.dmx_vals[i - 1]
                }
            }
        }
    }
}
