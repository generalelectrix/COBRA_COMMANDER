//! Some stateful controls for arrays of things that can strobe.
//!
//! Provides flash patterns, including sequences and true randomness.

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
