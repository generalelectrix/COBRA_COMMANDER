//! Some stateful controls for arrays of things that can strobe.
//!
//! Provides flash patterns, including sequences and true randomness.
use anyhow::{Result, anyhow, bail, ensure};
use log::error;
use rand::prelude::*;

pub type CellIndex = usize;
pub type ChaseIndex = usize;

/// Store the current flash state of a strobe array.
pub struct FlashState<const N: usize> {
    cells: [Option<Flash>; N],
    /// How many frames should we leave a flash on?
    flash_len: u8,
}

impl<const N: usize> Default for FlashState<N> {
    fn default() -> Self {
        FlashState {
            cells: [None; N],
            flash_len: 1,
        }
    }
}

impl<const N: usize> FlashState<N> {
    pub fn set(&mut self, cell: CellIndex) {
        if cell >= N {
            error!("strobe cell index {cell} out of range.");
            return;
        }
        self.cells[cell] = Some(Flash::default());
    }

    /// Age all of the flashes and clear them if they are done.
    pub fn update(&mut self, n_frames: u8) {
        for flash in &mut self.cells {
            if let Some(f) = flash {
                f.age += n_frames;
                if f.age >= self.flash_len {
                    *flash = None;
                }
            }
        }
    }

    /// Get a the cell state as a slice.
    pub fn cells(&self) -> &[Option<Flash>; N] {
        &self.cells
    }
}

/// A strobe "flash" event that has been on for some number of frames.
#[derive(Debug, Default, Copy, Clone)]
pub struct Flash {
    /// How many DMX frames has this flash been on for?
    age: u8,
}

/// Define methods required by strobe chases.
pub trait Chase<const N: usize> {
    /// Add flashes into the provided state corresponding to the next chase step.
    /// Update the state of the chase to the next step.
    /// If reverse is true, roll the chase backwards if possible.
    fn set_next(&mut self, reverse: bool, state: &mut FlashState<N>);

    /// Reset this chase to the beginning.
    fn reset(&mut self);
}

/// Handle flash state and maintain chases.
pub struct Flasher<const N: usize> {
    state: FlashState<N>,
    selected_chase: ChaseIndex,
    selected_multiplier: usize,
    /// If true, patches of this strobe array can handle mirroring - in other
    /// words, they do not physically have complete radial symmetry.
    can_mirror: bool,
    chases: Chases<N>,
}

impl<const N: usize> Flasher<N> {
    pub fn new(chases: Chases<N>, can_mirror: bool) -> Self {
        Self {
            state: Default::default(),
            selected_chase: Default::default(),
            selected_multiplier: 0,
            can_mirror,
            chases,
        }
    }
}

fn render_state_iter<'a>(
    iter: impl Iterator<Item = &'a Option<Flash>>,
    intensity: u8,
    dmx_buf: &mut [u8],
) {
    for (state, chan) in iter.zip(dmx_buf.iter_mut()) {
        *chan = if state.is_some() { intensity } else { 0 };
    }
}

impl<const N: usize> Flasher<N> {
    pub fn len(&self) -> usize {
        self.chases.chase_count()
    }

    pub fn render(
        &self,
        group_controls: &crate::fixture::FixtureGroupControls,
        intensity: u8,
        dmx_buf: &mut [u8],
    ) {
        if self.can_mirror && group_controls.mirror {
            render_state_iter(self.state.cells().iter().rev(), intensity, dmx_buf);
        } else {
            render_state_iter(self.state.cells().iter(), intensity, dmx_buf);
        }
    }

    pub fn update(
        &mut self,
        trigger_flash: bool,
        selected_chase: ChaseIndex,
        selected_multiplier: usize,
        reverse: bool,
    ) {
        self.state.update(1);

        let reset = selected_chase != self.selected_chase
            || selected_multiplier != self.selected_multiplier;
        if reset {
            self.selected_chase = selected_chase;
            self.selected_multiplier = selected_multiplier;
            if let Err(err) = self.chases.reset(selected_chase, selected_multiplier) {
                error!("{err}");
            };
        }

        if trigger_flash {
            if let Err(err) = self.chases.next(
                self.selected_chase,
                self.selected_multiplier,
                reverse,
                &mut self.state,
            ) {
                error!("{err}");
            };
        }
    }
}

/// Abstract over differently-sized flashers.
pub trait UnsizedFlasher {
    fn len(&self) -> usize;
    fn cells(&self) -> &[Option<Flash>];
    fn update(
        &mut self,
        trigger_flash: bool,
        selected_chase: ChaseIndex,
        selected_multiplier_index: usize,
        reverse: bool,
    );
}

impl<const N: usize> UnsizedFlasher for Flasher<N> {
    fn len(&self) -> usize {
        self.len()
    }

    fn cells(&self) -> &[Option<Flash>] {
        &self.state.cells()[..]
    }

    fn update(
        &mut self,
        trigger_flash: bool,
        selected_chase: ChaseIndex,
        selected_multiplier_index: usize,
        reverse: bool,
    ) {
        self.update(
            trigger_flash,
            selected_chase,
            selected_multiplier_index,
            reverse,
        );
    }
}

pub struct Chases<const N: usize>(Vec<ChaseSet<N>>);

struct ChaseSet<const N: usize> {
    pub multiplier: u8,
    pub chases: Vec<Box<dyn Chase<N>>>,
}

impl<const N: usize> ChaseSet<N> {
    pub fn new(multiplier: u8) -> Self {
        Self {
            multiplier,
            chases: vec![],
        }
    }

    pub fn len(&self) -> usize {
        self.chases.len()
    }

    /// Add a chase, using even offsets to apply this set's multiplier.
    pub fn add_with_mult_auto(&mut self, chase: impl Chase<N> + 'static + Clone) -> Result<()> {
        let stride = N / self.multiplier as usize;
        match self.multiplier {
            1 => self.chases.push(Box::new(chase)),
            2 => self.chases.push(Box::new(Lockstep::new(
                chase.clone(),
                chase.clone(),
                stride,
            ))),
            3 => {
                let pair = Lockstep::new(chase.clone(), chase.clone(), stride);
                self.chases
                    .push(Box::new(Lockstep::new(pair, chase.clone(), stride * 2)));
            }
            4 => {
                let pair = Lockstep::new(chase.clone(), chase.clone(), stride);
                self.chases
                    .push(Box::new(Lockstep::new(pair.clone(), pair, stride * 2)));
            }
            bad => bail!("unsupported chase set multipler {bad}"),
        }
        Ok(())
    }

    /// Add non-repeating random, taking multiplier segments per flash.
    pub fn add_random(&mut self) {
        self.chases
            .push(Box::new(RandomPattern::<N>::take(self.multiplier)));
    }
}

impl<const N: usize> Chases<N> {
    pub fn new(multipliers: &[u8]) -> Result<Self> {
        ensure!(multipliers[0] == 1);
        for &m in multipliers {
            ensure!(m > 0, "cannot use a strobe array chase multiplier of 0");
            ensure!(
                N % m as usize == 0,
                "strobe array with cell count {N} cannot divide evenly by multiplier {m}"
            );
        }
        Ok(Self(
            multipliers.iter().copied().map(ChaseSet::new).collect(),
        ))
    }

    pub fn chase_count(&self) -> usize {
        self.0.first().map(ChaseSet::len).unwrap_or_default()
    }

    /// Add a chase, automatically creating all multipliers using Lockstep.
    pub fn add_auto_mult(&mut self, chase: impl Chase<N> + 'static + Clone) -> Result<()> {
        for chase_set in &mut self.0 {
            chase_set.add_with_mult_auto(chase.clone())?;
        }
        Ok(())
    }

    /// Add a random chase, automatically creating all multipliers.
    pub fn add_auto_random(&mut self) {
        for chase_set in &mut self.0 {
            chase_set.add_random();
        }
    }

    /// Add a "flash all" at all multipliers.
    pub fn add_all(&mut self) {
        for chase_set in &mut self.0 {
            chase_set.chases.push(Box::new(PatternArray::all()));
        }
    }

    #[expect(dead_code)]
    /// Add a chase directly into a multiplier's collection.
    ///
    /// Use caution - we should always maintain the same number of chases for
    /// all multipliers.
    pub fn add_for_mult(
        &mut self,
        multiplier_index: usize,
        chase: impl Chase<N> + 'static,
    ) -> Result<()> {
        self.chase_set_mut(multiplier_index)?
            .chases
            .push(Box::new(chase));
        Ok(())
    }

    /// Advance the chase, and apply the next step to the provided state.
    pub fn next(
        &mut self,
        i: ChaseIndex,
        multiplier_index: usize,
        reverse: bool,
        state: &mut FlashState<N>,
    ) -> Result<()> {
        self.chase_mut(multiplier_index, i)?
            .set_next(reverse, state);
        Ok(())
    }

    /// Reset the specified chase.
    pub fn reset(&mut self, i: ChaseIndex, multiplier_index: usize) -> Result<()> {
        self.chase_mut(multiplier_index, i)?.reset();
        Ok(())
    }

    fn chase_set_mut(&mut self, multiplier_index: usize) -> Result<&mut ChaseSet<N>> {
        let n_mult = self.0.len();
        self.0
            .get_mut(multiplier_index)
            .ok_or_else(|| anyhow!("multiplier index {multiplier_index} out of range (> {n_mult})"))
    }

    fn chase_mut(
        &mut self,
        multiplier_index: usize,
        i: ChaseIndex,
    ) -> Result<&mut Box<dyn Chase<N>>> {
        let set = self.chase_set_mut(multiplier_index)?;
        let n_chase = set.len();
        set.chases
            .get_mut(i)
            .ok_or_else(|| anyhow!("chase index {i} out of range (> {n_chase})"))
    }
}

/// A determinisitc sequence of cells.
#[derive(Clone)]
pub struct PatternArray<const P: usize, const N: usize> {
    items: Vec<[CellIndex; P]>,
    next_item: usize,
}

impl<const P: usize, const N: usize> PatternArray<P, N> {
    pub fn new(items: Vec<[CellIndex; P]>) -> Self {
        for pattern in &items {
            for cell in pattern {
                assert!(*cell < N, "{cell} >= {N}");
            }
        }
        Self {
            items,
            next_item: 0,
        }
    }
}

impl<const N: usize> PatternArray<N, N> {
    pub fn all() -> Self {
        let mut cells = [0usize; N];
        for (i, c) in cells.iter_mut().enumerate() {
            *c = i;
        }
        Self::new(vec![cells])
    }
}

impl<const N: usize> PatternArray<1, N> {
    pub fn singles(cells: impl Iterator<Item = CellIndex>) -> Self {
        Self::new(cells.map(|i| [i]).collect())
    }
}

impl<const N: usize> PatternArray<2, N> {
    pub fn doubles(cells: impl Iterator<Item = (CellIndex, CellIndex)>) -> Self {
        Self::new(cells.map(|(i0, i1)| [i0, i1]).collect())
    }
}

impl<const P: usize, const N: usize> Chase<N> for PatternArray<P, N> {
    fn set_next(&mut self, reverse: bool, state: &mut FlashState<N>) {
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

/// A non-repeating random chase that flashes each cell once before re-shuffling.
#[derive(Clone)]
pub struct RandomPattern<const N: usize> {
    rng: SmallRng,
    cells: [u8; N],
    next_item: usize,
    /// How many items should we take at a time?
    /// Needs to be an even divisor of cell count to always show this many;
    /// otherwise, we will reset in the middle of a step and potentially show
    /// fewer flashes.
    take: u8,
}

impl<const C: usize> RandomPattern<C> {
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

    fn set_next_single(&mut self, state: &mut FlashState<C>) {
        if self.next_item >= self.cells.len() {
            self.reset();
        }
        state.set(self.cells[self.next_item] as usize);
        self.next_item += 1;
    }
}

impl<const C: usize> Chase<C> for RandomPattern<C> {
    fn reset(&mut self) {
        self.cells.shuffle(&mut self.rng);
        self.next_item = 0;
    }

    fn set_next(&mut self, _reverse: bool, state: &mut FlashState<C>) {
        for _ in 0..self.take {
            self.set_next_single(state);
        }
    }
}

/// Combine two chases into one, by offsetting one chase by offset.
///
/// The chases then iterate in lockstep.
#[derive(Clone)]
pub struct Lockstep<const C: usize, C0: Chase<C>, C1: Chase<C>> {
    c0: C0,
    c1: C1,
    offset: usize,
}

impl<const C: usize, C0: Chase<C>, C1: Chase<C>> Lockstep<C, C0, C1> {
    pub fn new(c0: C0, c1: C1, offset: usize) -> Self {
        Self { c0, c1, offset }
    }
}

impl<const C: usize, C0: Chase<C>, C1: Chase<C>> Chase<C> for Lockstep<C, C0, C1> {
    fn reset(&mut self) {
        self.c0.reset();
        self.c1.reset();
        // use a fake state to offset the second chase
        let mut dummy = FlashState::default();
        for _ in 0..self.offset {
            self.c1.set_next(false, &mut dummy);
        }
    }

    fn set_next(&mut self, reverse: bool, state: &mut FlashState<C>) {
        self.c0.set_next(reverse, state);
        self.c1.set_next(reverse, state);
    }
}
