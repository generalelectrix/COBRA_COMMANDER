//! Profile for the Monoprice "Flash Bang" 5-ring LED strobe.
//!
//! The profile is designed to use the direct-control 5-channel mode where
//! the brightness of each LED ring is directly controlled. There is also a
//! special 10-channel mode provided for using a pair of these fixtures where
//! the patterns can be extended over both arrays for additional effects.
use log::error;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

#[derive(EmitState, Control)]
#[strobe(Short)]
pub struct FlashBang {
    #[channel_control]
    #[animate]
    intensity: ChannelKnobUnipolar<Unipolar<()>>,
    chase: IndexedSelect<()>,
    reverse: Bool<()>,
    #[skip_emit]
    #[skip_control]
    flasher: Box<dyn UnsizedFlasher>,
}

#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct GroupOptions {
    #[serde(default)]
    paired: bool,
}

impl PatchFixture for FlashBang {
    const NAME: FixtureType = FixtureType("FlashBang");
    type GroupOptions = GroupOptions;
    type PatchOptions = NoOptions;

    fn new(options: Self::GroupOptions) -> Self {
        let flasher = if options.paired {
            Box::new(paired_flasher()) as Box<dyn UnsizedFlasher>
        } else {
            Box::new(single_flasher())
        };

        Self {
            intensity: Unipolar::new("Intensity", ())
                .at(UnipolarFloat::new(0.1))
                .with_channel_knob(0),
            chase: IndexedSelect::new("Chase", flasher.len(), false, ()),
            reverse: Bool::new_off("Reverse", ()),
            flasher,
        }
    }

    fn new_patch(options: Self::GroupOptions, _: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: if options.paired { 10 } else { 5 },
            render_mode: None,
        }
    }
}

impl Update for FlashBang {
    fn update(&mut self, update: FixtureGroupUpdate, _dt: std::time::Duration) {
        self.flasher
            .update(update.flash_now, self.chase.selected(), self.reverse.val());
    }
}

impl AnimatedFixture for FlashBang {
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
            for _ in 0..self.flasher.cells().len() {
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
        for (flash, chan) in self.flasher.cells().iter().zip(dmx_buf.iter_mut()) {
            *chan = if flash.is_some() { intensity } else { 0 };
            group_controls.preview.intensity_u8(*chan);
        }
    }
}

register_patcher!(FlashBang);

/// Abstract over 5-cell vs 10-cell flashers for different render modes.
trait UnsizedFlasher {
    fn len(&self) -> usize;
    fn cells(&self) -> &[Option<Flash>];
    fn update(&mut self, trigger_flash: bool, selected_chase: ChaseIndex, reverse: bool);
}

fn single_flasher() -> Flasher<5> {
    const CELLS: usize = 5;
    let mut f: Flasher<CELLS> = Default::default();
    // all
    f.add_chase(PatternArray::all());
    // single pulse
    f.add_chase(PatternArray::singles(0..CELLS));
    // single pulse bounce
    f.add_chase(PatternArray::singles(
        (0..CELLS).chain((1..CELLS - 1).rev()),
    ));
    // random
    f.add_chase(RandomPattern::<CELLS>::take(1));
    f
}

fn paired_flasher() -> Flasher<10> {
    const CELLS: usize = 10;
    let mut f: Flasher<CELLS> = Default::default();
    // all
    f.add_chase(PatternArray::all());
    // single pulse
    f.add_chase(PatternArray::singles(0..CELLS));
    // single pulse bounce
    f.add_chase(PatternArray::singles(
        (0..CELLS).chain((1..CELLS - 1).rev()),
    ));
    // random
    f.add_chase(RandomPattern::<CELLS>::take(1));
    f
}
#[derive(Default)]
struct Flasher<const N: usize> {
    state: FlashState<N>,
    selected_chase: ChaseIndex,
    chases: Vec<Box<dyn Chase<N>>>,
}

impl<const N: usize> UnsizedFlasher for Flasher<N> {
    fn len(&self) -> usize {
        self.chases.len()
    }

    fn cells(&self) -> &[Option<Flash>] {
        &self.state.cells()[..]
    }

    fn update(&mut self, trigger_flash: bool, selected_chase: ChaseIndex, reverse: bool) {
        self.state.update(1);

        let reset = selected_chase != self.selected_chase;
        if reset {
            self.selected_chase = selected_chase;
            self.reset();
        }

        if trigger_flash {
            self.flash_next(reverse);
        }
    }
}

impl<const N: usize> Flasher<N> {
    pub fn add_chase(&mut self, c: impl Chase<N> + 'static) {
        self.chases.push(Box::new(c));
    }

    fn reset(&mut self) {
        let Some(chase) = self.chases.get_mut(self.selected_chase) else {
            error!(
                "Selected Flash Bang chase {} out of range.",
                self.selected_chase
            );
            return;
        };
        chase.reset();
    }

    fn flash_next(&mut self, reverse: bool) {
        let Some(chase) = self.chases.get_mut(self.selected_chase) else {
            error!(
                "Selected Flash Bang chase {} out of range.",
                self.selected_chase
            );
            return;
        };
        chase.set_next(reverse, &mut self.state);
    }
}
