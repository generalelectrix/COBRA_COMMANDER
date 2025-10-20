//! Profile for the Big Bar, the American DJ Freq Strobe 16.
//!
//! TODO: migrate strobe mechanism to the global strobe clock.
use std::{iter::zip, time::Duration};

use log::error;
use rand::prelude::*;

use crate::fixture::prelude::*;

const CELL_COUNT: usize = 16;

#[derive(EmitState, Control, PatchFixture)]
#[channel_count = 18]
pub struct FreqStrobe {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    run: Bool<()>,
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
            run: Bool::new_off("Run", ()),
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
        let run = master_controls.strobe_state.strobe_on && self.run.val();
        let rate = if master_controls.strobe_state.use_master_rate {
            master_controls.strobe_state.rate
        } else {
            self.rate.control.val()
        };
        self.flasher.update(
            dt,
            run,
            rate,
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

type CellIndex = usize;
type ChaseIndex = usize;

#[derive(Default)]
struct Flasher {
    state: FlashState,
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

impl Flasher {
    pub fn len(&self) -> usize {
        self.chases.len()
    }

    pub fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        if group_controls.mirror {
            render_state_iter(self.state.cells.iter().rev(), dmx_buf);
        } else {
            render_state_iter(self.state.cells.iter(), dmx_buf);
        }
    }

    pub fn update(
        &mut self,
        dt: Duration,
        run: bool,
        rate: UnipolarFloat,
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

        if run && self.last_flash_age >= interval_from_rate(rate) {
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

fn interval_from_rate(rate: UnipolarFloat) -> Duration {
    // lowest rate: 1 flash/sec => 1 sec interval
    // highest rate: 50 flash/sec => 20 ms interval
    // use exact frame intervals
    // FIXME: this should depend on the show framerate explicitly.
    // FIXME: DMX is actually limited to 40 fps, not 50
    let raw_interval = (100. / (rate.val() + 0.09)) as u64 - 70;
    let coerced_interval = ((raw_interval / 20) * 20).max(20);
    Duration::from_millis(coerced_interval)
}

struct FlashState {
    cells: [Option<Flash>; CELL_COUNT],
    flash_len: Duration,
}

impl Default for FlashState {
    fn default() -> Self {
        FlashState {
            cells: Default::default(),
            flash_len: Duration::from_millis(40),
        }
    }
}

impl FlashState {
    pub fn set(&mut self, cell: CellIndex) {
        if cell >= CELL_COUNT {
            error!("FreqStrobe cell index {cell} out of range.");
            return;
        }
        self.cells[cell] = Some(Flash::default());
    }

    pub fn update(&mut self, dt: Duration) {
        for flash in &mut self.cells {
            if let Some(f) = flash {
                f.age += dt;
                if f.age >= self.flash_len {
                    *flash = None;
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct Flash {
    age: Duration,
}

struct Chases {
    singles: Vec<Box<dyn Chase>>,
    doubles: Vec<Box<dyn Chase>>,
    quads: Vec<Box<dyn Chase>>,
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
        p.add_single(RandomPattern::take(1));
        // random pairs, non-repeating until all cells flash
        p.add_double(RandomPattern::take(2));
        // random quads, non-repeating until all cells flash
        p.add_quad(RandomPattern::take(4));
        p
    }
}

impl Chases {
    pub fn len(&self) -> usize {
        self.singles.len()
    }

    /// Add a chase, automatically creating multipliers using Lockstep.
    pub fn add_auto_mult(&mut self, chase: impl Chase + 'static + Clone) {
        self.add_single(chase.clone());
        let double = Lockstep {
            c0: chase.clone(),
            c1: chase.clone(),
            offset: 8,
        };
        self.add_double(double.clone());
        self.add_quad(Lockstep {
            c0: double.clone(),
            c1: double.clone(),
            offset: 4,
        });
    }

    /// Add a single-flash chase.
    fn add_single(&mut self, chase: impl Chase + 'static) {
        self.singles.push(Box::new(chase) as Box<dyn Chase>);
    }

    /// Add a double-flash (2x mult) chase.
    fn add_double(&mut self, chase: impl Chase + 'static) {
        self.doubles.push(Box::new(chase) as Box<dyn Chase>);
    }

    /// Add a quad-flash (4x mult) chase.
    fn add_quad(&mut self, chase: impl Chase + 'static) {
        self.quads.push(Box::new(chase) as Box<dyn Chase>);
    }

    pub fn next(
        &mut self,
        i: ChaseIndex,
        multiplier: usize,
        reverse: bool,
        state: &mut FlashState,
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

trait Chase {
    /// Add flashes into the provided state corresponding to the next chase step.
    /// Update the state of the chase to the next step.
    /// If reverse is true, roll the chase backwards if possible.
    fn set_next(&mut self, reverse: bool, state: &mut FlashState);

    /// Reset this chase to the beginning.
    fn reset(&mut self);
}

#[derive(Clone)]
struct PatternArray<const N: usize> {
    items: Vec<[CellIndex; N]>,
    next_item: usize,
}

impl<const N: usize> PatternArray<N> {
    pub fn new(items: Vec<[CellIndex; N]>) -> Self {
        for pattern in &items {
            for cell in pattern {
                assert!(*cell < CELL_COUNT);
            }
        }
        Self {
            items,
            next_item: 0,
        }
    }
}

impl PatternArray<1> {
    pub fn singles(cells: impl Iterator<Item = CellIndex>) -> Self {
        Self::new(cells.map(|i| [i]).collect())
    }
}

impl PatternArray<2> {
    pub fn doubles(cells: impl Iterator<Item = (CellIndex, CellIndex)>) -> Self {
        Self::new(cells.map(|(i0, i1)| [i0, i1]).collect())
    }
}

impl<const N: usize> Chase for PatternArray<N> {
    fn set_next(&mut self, reverse: bool, state: &mut FlashState) {
        for cell in self.items[self.next_item] {
            state.set(cell);
        }
        if reverse {
            if self.next_item == 0 {
                self.next_item = self.items.len() - 1;
            } else {
                self.next_item -= 1;
            }
        } else {
            self.next_item += 1;
            self.next_item %= self.items.len();
        }
    }

    fn reset(&mut self) {
        self.next_item = 0;
    }
}

#[derive(Clone)]
struct RandomPattern {
    rng: SmallRng,
    cells: [u8; CELL_COUNT],
    next_item: usize,
    /// How many items should we take at a time?
    /// Needs to be an even divisor of cell count, so basically 1, 2, or 4.
    take: u8,
}

impl RandomPattern {
    pub fn take(take: u8) -> Self {
        let mut rp = Self {
            rng: SmallRng::seed_from_u64(123456789),
            cells: core::array::from_fn(|i| i as u8),
            next_item: 0,
            take,
        };
        rp.reset();
        rp
    }

    fn set_next_single(&mut self, state: &mut FlashState) {
        if self.next_item >= self.cells.len() {
            self.reset();
        }
        state.set(self.cells[self.next_item] as usize);
        self.next_item += 1;
    }
}

impl Chase for RandomPattern {
    fn reset(&mut self) {
        self.cells.shuffle(&mut self.rng);
        self.next_item = 0;
    }

    fn set_next(&mut self, _reverse: bool, state: &mut FlashState) {
        for _ in 0..self.take {
            self.set_next_single(state);
        }
    }
}

#[derive(Clone)]
struct Lockstep<C0: Chase, C1: Chase> {
    c0: C0,
    c1: C1,
    offset: usize,
}

impl<C0: Chase, C1: Chase> Chase for Lockstep<C0, C1> {
    fn reset(&mut self) {
        self.c0.reset();
        self.c1.reset();
        // use a fake state to offset the second chase
        let mut dummy = FlashState::default();
        for _ in 0..self.offset {
            self.c1.set_next(false, &mut dummy);
        }
    }

    fn set_next(&mut self, reverse: bool, state: &mut FlashState) {
        self.c0.set_next(reverse, state);
        self.c1.set_next(reverse, state);
    }
}
