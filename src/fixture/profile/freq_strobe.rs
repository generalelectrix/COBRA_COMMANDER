//! Profile for the Big Bar, the American DJ Freq Strobe 16.
//!
//! TODO: merge this and the profile for Flash Bang.
use anyhow::Result;
use anyhow::anyhow;
use anyhow::ensure;
use log::error;
use std::iter::zip;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

const CELL_COUNT: usize = 16;

#[derive(EmitState, Control, PatchFixture)]
#[channel_count = 16]
#[strobe(Short)]
pub struct FreqStrobe {
    #[channel_control]
    #[animate]
    intensity: ChannelKnobUnipolar<Unipolar<()>>,
    pattern: IndexedSelect<()>,
    multiplier: IndexedSelect<()>,
    reverse: Bool<()>,
    #[skip_emit]
    #[skip_control]
    flasher: Flasher,
}

impl Default for FreqStrobe {
    fn default() -> Self {
        let flasher = Flasher::new(create_chases().unwrap());
        Self {
            intensity: Unipolar::new("Intensity", ())
                .at_full()
                .with_channel_knob(0),
            pattern: IndexedSelect::new("Chase", flasher.len(), false, ()),
            multiplier: IndexedSelect::new("Multiplier", 3, false, ()),
            reverse: Bool::new_off("Reverse", ()),
            flasher,
        }
    }
}

impl Update for FreqStrobe {
    fn update(&mut self, update: FixtureGroupUpdate, _dt: std::time::Duration) {
        self.flasher.update(
            update.flash_now,
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
        // If strobing is disabled, blackout.
        if !group_controls.strobe_enabled {
            dmx_buf.fill(0);
            for _ in 0..CELL_COUNT {
                group_controls.preview.intensity_u8(0);
            }
            return;
        }
        // Scale the intensity by the master strobe intensity.
        let intensity = unipolar_to_range(
            0,
            255,
            self.intensity
                .control
                .val_with_anim(animation_vals.filter(&AnimationTarget::Intensity))
                * group_controls.strobe_clock().intensity(),
        );
        self.flasher.render(group_controls, intensity, dmx_buf);
        for &i in &*dmx_buf {
            group_controls.preview.intensity_u8(i);
        }
    }
}

fn two_flash_spread() -> impl DoubleEndedIterator<Item = (CellIndex, CellIndex)> {
    zip((0..CELL_COUNT / 2).rev(), CELL_COUNT / 2..CELL_COUNT)
}

fn create_chases() -> Result<Chases> {
    let mut p = Chases::new(&[1, 2, 4])?;
    // single pulse 1-16
    p.add_auto_mult(PatternArray::singles(0..CELL_COUNT))?;
    // single pulse bounce
    p.add_auto_mult(PatternArray::singles(
        (0..CELL_COUNT).chain((1..CELL_COUNT - 1).rev()),
    ))?;
    // two flash spread from middle
    p.add_auto_mult(PatternArray::doubles(two_flash_spread()))?;
    // two flash bounce, starting out
    p.add_auto_mult(PatternArray::doubles(
        two_flash_spread().chain(two_flash_spread().rev().skip(1).take(6)),
    ))?;

    p.add_auto_random();
    Ok(p)
}

struct Flasher {
    state: FlashState<CELL_COUNT>,
    selected_chase: ChaseIndex,
    selected_multiplier: usize,
    chases: Chases,
}

impl Flasher {
    pub fn new(chases: Chases) -> Self {
        Self {
            state: Default::default(),
            selected_chase: Default::default(),
            selected_multiplier: 0,
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

impl Flasher {
    pub fn len(&self) -> usize {
        self.chases.chase_count()
    }

    pub fn render(&self, group_controls: &FixtureGroupControls, intensity: u8, dmx_buf: &mut [u8]) {
        if group_controls.mirror {
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

pub struct Chases(Vec<ChaseSet>);

struct ChaseSet {
    pub multiplier: u8,
    pub chases: Vec<Box<dyn Chase<CELL_COUNT>>>,
}

impl ChaseSet {
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
    pub fn add_with_mult_auto(
        &mut self,
        chase: impl Chase<CELL_COUNT> + 'static + Clone,
    ) -> Result<()> {
        let stride = CELL_COUNT / self.multiplier as usize;
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
            .push(Box::new(RandomPattern::<CELL_COUNT>::take(self.multiplier)));
    }
}

impl Chases {
    pub fn new(multipliers: &[u8]) -> Result<Self> {
        ensure!(multipliers[0] == 1);
        for &m in multipliers {
            ensure!(m > 0, "cannot use a strobe array chase multiplier of 0");
            ensure!(
                CELL_COUNT % m as usize == 0,
                "strobe array with cell count {CELL_COUNT} cannot divide evenly by multiplier {m}"
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
    pub fn add_auto_mult(&mut self, chase: impl Chase<CELL_COUNT> + 'static + Clone) -> Result<()> {
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

    /// Add a chase directly into a multiplier's collection.
    ///
    /// Use caution - we should always maintain the same number of chases for
    /// all multipliers.
    pub fn add_for_mult(
        &mut self,
        multiplier_index: usize,
        chase: impl Chase<CELL_COUNT> + 'static,
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
        state: &mut FlashState<CELL_COUNT>,
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

    fn chase_set_mut(&mut self, multiplier_index: usize) -> Result<&mut ChaseSet> {
        let n_mult = self.0.len();
        self.0
            .get_mut(multiplier_index)
            .ok_or_else(|| anyhow!("multiplier index {multiplier_index} out of range (> {n_mult})"))
    }

    fn chase_mut(
        &mut self,
        multiplier_index: usize,
        i: ChaseIndex,
    ) -> Result<&mut Box<dyn Chase<CELL_COUNT>>> {
        let set = self.chase_set_mut(multiplier_index)?;
        let n_chase = set.len();
        set.chases
            .get_mut(i)
            .ok_or_else(|| anyhow!("chase index {i} out of range (> {n_chase})"))
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_chases() {
        super::create_chases().unwrap();
    }
}
