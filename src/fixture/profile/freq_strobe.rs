//! Profile for the Big Bar, the American DJ Freq Strobe 16.
//!
//! TODO: migrate strobe mechanism to the global strobe clock.
use std::{iter::zip, time::Duration};

use log::error;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

const CELL_COUNT: usize = 16;

#[derive(EmitState, Control, PatchFixture)]
#[channel_count = 18]
#[strobe]
pub struct FreqStrobe {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    follow_master: Bool<()>,
    #[channel_control]
    rate: ChannelKnobUnipolar<Unipolar<()>>,
    pattern: IndexedSelect<()>,
    multiplier: IndexedSelect<()>,
    reverse: Bool<()>,
    #[skip_emit]
    #[skip_control]
    flasher: Flasher,
}

impl Default for FreqStrobe {
    fn default() -> Self {
        let flasher = Flasher::default();
        Self {
            dimmer: Unipolar::channel("Dimmer", 16, 1, 255).with_channel_level(),
            // strobe: Strobe::channel("Strobe", 17, 9, 131, 0),
            follow_master: Bool::new_on("FollowMaster", ()),
            rate: Unipolar::new("Rate", ()).with_channel_knob(0),
            pattern: IndexedSelect::new("Chase", flasher.len(), false, ()),
            multiplier: IndexedSelect::new("Multiplier", 3, false, ()),
            reverse: Bool::new_off("Reverse", ()),
            flasher,
        }
    }
}

impl Update for FreqStrobe {
    fn update(&mut self, master_controls: &MasterControls, dt: std::time::Duration) {
        let update = if self.follow_master.val() {
            UpdateBehavior::Master(master_controls.strobe_state.ticked)
        } else {
            UpdateBehavior::Internal(self.rate.control.val())
        };
        self.flasher.update(
            dt,
            master_controls.strobe_state.strobe_on,
            update,
            self.pattern.selected(),
            self.multiplier.selected(),
            self.reverse.val(),
        );
    }
}

impl AnimatedFixture for FreqStrobe {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.flasher.render(group_controls, dmx_buf);
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
    }
}

#[derive(Default)]
struct Flasher {
    state: FlashState<CELL_COUNT>,
    selected_chase: ChaseIndex,
    selected_multiplier: usize,
    chases: Chases,
    last_flash_age: Duration,
}

fn render_state_iter<'a>(iter: impl Iterator<Item = &'a Option<Flash>>, dmx_buf: &mut [u8]) {
    for (state, chan) in iter.zip(dmx_buf.iter_mut()) {
        *chan = if state.is_some() { 255 } else { 0 }
    }
}

enum UpdateBehavior {
    /// Update flasher state using a continuous rate parameter.
    Internal(UnipolarFloat),
    /// Master control active - trigger a flash if true.
    Master(bool),
}

impl Flasher {
    pub fn len(&self) -> usize {
        self.chases.len()
    }

    pub fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        if group_controls.mirror {
            render_state_iter(self.state.cells().iter().rev(), dmx_buf);
        } else {
            render_state_iter(self.state.cells().iter(), dmx_buf);
        }
    }

    pub fn update(
        &mut self,
        dt: Duration,
        run: bool,
        behavior: UpdateBehavior,
        selected_chase: ChaseIndex,
        selected_multiplier: usize,
        reverse: bool,
    ) {
        self.state.update(dt);
        self.last_flash_age += dt;

        let reset = selected_chase != self.selected_chase
            || selected_multiplier != self.selected_multiplier;
        if reset {
            self.selected_chase = selected_chase;
            self.selected_multiplier = selected_multiplier;
            self.chases.reset(selected_chase, selected_multiplier);
        }

        let trigger_flash = match behavior {
            UpdateBehavior::Internal(rate) => self.last_flash_age >= interval_from_rate(rate),

            UpdateBehavior::Master(flash) => flash,
        };

        if run && trigger_flash {
            self.chases.next(
                self.selected_chase,
                self.selected_multiplier,
                reverse,
                &mut self.state,
            );
            self.last_flash_age = Duration::ZERO;
        }
    }
}

/// Convert a rate scale control into a duration, coercing all values into
/// integer numbers of frames to avoid aliasing.
fn interval_from_rate(rate: UnipolarFloat) -> Duration {
    // lowest rate: 1 flash/sec => 1 sec interval
    // highest rate: 40 flash/sec => 25 ms interval
    // use exact frame intervals
    // FIXME: this should depend on the show framerate explicitly.
    let raw_interval = (100. / (rate.val() + 0.09)) as u64 - 66;
    let coerced_interval = ((raw_interval / 25) * 25).max(25);
    Duration::from_millis(coerced_interval)
}

struct Chases {
    singles: Vec<Box<dyn Chase<CELL_COUNT>>>,
    doubles: Vec<Box<dyn Chase<CELL_COUNT>>>,
    quads: Vec<Box<dyn Chase<CELL_COUNT>>>,
}

fn two_flash_spread() -> impl DoubleEndedIterator<Item = (CellIndex, CellIndex)> {
    zip((0..CELL_COUNT / 2).rev(), CELL_COUNT / 2..CELL_COUNT)
}

impl Default for Chases {
    fn default() -> Self {
        let mut p = Self {
            singles: vec![],
            doubles: vec![],
            quads: vec![],
        };
        // single pulse 1-16
        p.add_auto_mult(PatternArray::singles(0..CELL_COUNT));
        // single pulse bounce
        p.add_auto_mult(PatternArray::singles(
            (0..CELL_COUNT).chain((1..CELL_COUNT - 1).rev()),
        ));
        // two flash spread from middle
        p.add_auto_mult(PatternArray::doubles(two_flash_spread()));
        // two flash bounce, starting out
        p.add_auto_mult(PatternArray::doubles(
            two_flash_spread().chain(two_flash_spread().rev().skip(1).take(6)),
        ));

        // random single pulses, non-repeating until all cells flash
        // added manually to always strobe the right number of patterns
        p.add_single(RandomPattern::<CELL_COUNT>::take(1));
        // random pairs, non-repeating until all cells flash
        p.add_double(RandomPattern::<CELL_COUNT>::take(2));
        // random quads, non-repeating until all cells flash
        p.add_quad(RandomPattern::<CELL_COUNT>::take(4));
        p
    }
}

impl Chases {
    pub fn len(&self) -> usize {
        self.singles.len()
    }

    /// Add a chase, automatically creating multipliers using Lockstep.
    pub fn add_auto_mult(&mut self, chase: impl Chase<CELL_COUNT> + 'static + Clone) {
        self.add_single(chase.clone());
        let double = Lockstep::new(chase.clone(), chase.clone(), 8);
        self.add_double(double.clone());
        self.add_quad(Lockstep::new(double.clone(), double.clone(), 4));
    }

    /// Add a single-flash chase.
    fn add_single(&mut self, chase: impl Chase<CELL_COUNT> + 'static) {
        self.singles
            .push(Box::new(chase) as Box<dyn Chase<CELL_COUNT>>);
    }

    /// Add a double-flash (2x mult) chase.
    fn add_double(&mut self, chase: impl Chase<CELL_COUNT> + 'static) {
        self.doubles
            .push(Box::new(chase) as Box<dyn Chase<CELL_COUNT>>);
    }

    /// Add a quad-flash (4x mult) chase.
    fn add_quad(&mut self, chase: impl Chase<CELL_COUNT> + 'static) {
        self.quads
            .push(Box::new(chase) as Box<dyn Chase<CELL_COUNT>>);
    }

    pub fn next(
        &mut self,
        i: ChaseIndex,
        multiplier: usize,
        reverse: bool,
        state: &mut FlashState<CELL_COUNT>,
    ) {
        let collection = match multiplier {
            0 => &mut self.singles,
            1 => &mut self.doubles,
            2 => &mut self.quads,
            _ => {
                error!("Selected FreqStrobe multiplier {multiplier} out of range.");
                return;
            }
        };
        let Some(chase) = collection.get_mut(i) else {
            error!("Selected FreqStrobe chase {i} out of range.");
            return;
        };
        chase.set_next(reverse, state);
    }

    pub fn reset(&mut self, i: ChaseIndex, multiplier: usize) {
        let collection = match multiplier {
            0 => &mut self.singles,
            1 => &mut self.doubles,
            2 => &mut self.quads,
            _ => {
                error!("Selected FreqStrobe multiplier {multiplier} out of range.");
                return;
            }
        };
        let Some(chase) = collection.get_mut(i) else {
            error!("Selected FreqStrobe chase {i} out of range.");
            return;
        };
        chase.reset();
    }
}
