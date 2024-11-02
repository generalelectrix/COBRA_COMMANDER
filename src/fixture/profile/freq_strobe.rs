//! Profile for the Big Bar, the American DJ Freq Strobe 16.
use std::time::Duration;

use log::error;

use crate::fixture::prelude::*;

const CELL_COUNT: u8 = 16;

#[derive(EmitState, Control)]
pub struct FreqStrobe {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    run: Bool<()>,
    rate: Unipolar<()>,
    pattern: IndexedSelect<()>,
    #[skip_emit]
    #[skip_control]
    flasher: Flasher,
}

impl Default for FreqStrobe {
    fn default() -> Self {
        let flasher = Flasher::default();
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 16).with_channel_level(),
            // strobe: Strobe::channel("Strobe", 17, 9, 131, 0),
            run: Bool::new_off("Run", ()),
            rate: Unipolar::new("Rate", ()),
            pattern: IndexedSelect::new("Pattern", flasher.len(), false, ()),
            flasher,
        }
    }
}

impl PatchAnimatedFixture for FreqStrobe {
    const NAME: FixtureType = FixtureType("FreqStrobe");
    fn channel_count(&self) -> usize {
        18
    }
}

impl ControllableFixture for FreqStrobe {
    fn update(&mut self, master_controls: &MasterControls, dt: std::time::Duration) {
        let master_strobe = master_controls.strobe();
        let run = master_strobe.state.on && self.run.val();
        let rate = if master_controls.strobe().use_master_rate {
            master_controls.strobe().state.rate
        } else {
            self.rate.val()
        };
        self.flasher.update(dt, run, rate, self.pattern.selected());
    }
}

impl AnimatedFixture for FreqStrobe {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.flasher.render(group_controls, dmx_buf);
        self.dimmer.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
    }
}

struct Flasher {
    state: [Option<Flash>; CELL_COUNT as usize],
    selected_chase: usize,
    chases: Chases,
    flash_len: Duration,
    last_flash_age: Duration,
}

impl Default for Flasher {
    fn default() -> Self {
        Self {
            state: Default::default(),
            selected_chase: 0,
            chases: Chases::default(),
            flash_len: Duration::from_millis(40),
            last_flash_age: Default::default(),
        }
    }
}

fn render_state_iter<'a>(iter: impl Iterator<Item = &'a Option<Flash>>, dmx_buf: &mut [u8]) {
    for (state, chan) in iter.zip(dmx_buf.iter_mut()) {
        *chan = if state.is_some() { 255 } else { 0 }
    }
}

impl Flasher {
    pub fn len(&self) -> usize {
        self.chases.0.len()
    }

    pub fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        if group_controls.mirror {
            render_state_iter(self.state.iter().rev(), dmx_buf);
        } else {
            render_state_iter(self.state.iter(), dmx_buf);
        }
    }

    pub fn update(&mut self, dt: Duration, run: bool, rate: UnipolarFloat, selected_chase: usize) {
        for flash in &mut self.state {
            if let Some(f) = flash {
                f.age += dt;
                if f.age >= self.flash_len {
                    *flash = None;
                }
            }
        }
        self.last_flash_age += dt;

        let reset = selected_chase != self.selected_chase;
        if reset {
            self.selected_chase = selected_chase;
            self.chases.reset(selected_chase);
        }

        if run && self.last_flash_age >= interval_from_rate(rate) {
            for cell_index in self.chases.next(self.selected_chase) {
                self.state[*cell_index as usize] = Some(Flash::default());
            }

            self.last_flash_age = Duration::ZERO;
        }
    }
}

fn interval_from_rate(rate: UnipolarFloat) -> Duration {
    // lowest rate: 1 flash/sec => 1 sec interval
    // highest rate: 50 flash/sec => 20 ms interval
    // use exact frame intervals
    // FIXME: this should depend on the show framerate explicitly.
    let raw_interval = (100. / (rate.val() + 0.09)) as u64 - 70;
    let coerced_interval = ((raw_interval / 20) * 20).max(20);
    Duration::from_millis(coerced_interval)
}

#[derive(Debug, Default)]
struct Flash {
    age: Duration,
}

struct Chases(Vec<Box<dyn Chase>>);

impl Default for Chases {
    fn default() -> Self {
        let mut p = Self(vec![]);
        // single pulse 1-16
        p.add(PatternArray::new((0..CELL_COUNT).map(|i| [i]).collect()));
        // single pulse 16-1
        p.add(PatternArray::new(
            (0..CELL_COUNT).rev().map(|i| [i]).collect(),
        ));
        p
    }
}

impl Chases {
    pub fn add(&mut self, p: impl Chase + 'static) {
        self.0.push(Box::new(p) as Box<dyn Chase>);
    }

    pub fn next(&mut self, i: usize) -> &[u8] {
        let Some(chase) = self.0.get_mut(i) else {
            error!("selected pattern {i} out of range");
            return &[];
        };
        chase.next()
    }

    pub fn reset(&mut self, i: usize) {
        let Some(chase) = self.0.get_mut(i) else {
            error!("selected pattern {i} out of range");
            return;
        };
        chase.reset();
    }
}

trait Chase {
    fn next(&mut self) -> &[u8];
    fn reset(&mut self);
}

struct PatternArray<const N: usize> {
    items: Vec<[u8; N]>,
    next_item: usize,
}

impl<const N: usize> PatternArray<N> {
    pub fn new(items: Vec<[u8; N]>) -> Self {
        Self {
            items,
            next_item: 0,
        }
    }
}

impl<const N: usize> Chase for PatternArray<N> {
    fn next(&mut self) -> &[u8] {
        let index = self.next_item;
        self.next_item += 1;
        self.next_item %= self.items.len();
        &self.items[index]
    }

    fn reset(&mut self) {
        self.next_item = 0;
    }
}
