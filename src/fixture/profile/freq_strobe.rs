//! Profile for the Big Bar, the American DJ Freq Strobe 16.
//!
//! TODO: merge this and the profile for Flash Bang.
use std::iter::zip;

use log::error;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

const CELL_COUNT: usize = 16;

#[derive(EmitState, Control, PatchFixture)]
#[channel_count = 16]
#[strobe]
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
        let flasher = Flasher::default();
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
    fn update(&mut self, master_controls: &MasterControls, _dt: std::time::Duration) {
        self.flasher.update(
            master_controls.strobe_state.flash_now,
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
                * group_controls.strobe().master_intensity,
        );
        self.flasher.render(group_controls, intensity, dmx_buf);
        for &i in &*dmx_buf {
            group_controls.preview.intensity_u8(i);
        }
    }
}

#[derive(Default)]
struct Flasher {
    state: FlashState<CELL_COUNT>,
    selected_chase: ChaseIndex,
    selected_multiplier: usize,
    chases: Chases,
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
        self.chases.len()
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
            self.chases.reset(selected_chase, selected_multiplier);
        }

        if trigger_flash {
            self.chases.next(
                self.selected_chase,
                self.selected_multiplier,
                reverse,
                &mut self.state,
            );
        }
    }
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
