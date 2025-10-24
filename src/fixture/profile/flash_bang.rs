//! Profile for the Monoprice "Flash Bang" 5-ring LED strobe.
//!
//! The profile is designed to use the direct-control 5-channel mode where
//! the brightness of each LED ring is directly controlled. There is also a
//! special 10-channel mode provided for using a pair of these fixtures where
//! the patterns can be extended over both arrays for additional effects.
use anyhow::Context;
use log::error;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

#[derive(EmitState, Control)]
#[strobe]
pub struct FlashBang {
    #[channel_control]
    #[animate]
    intensity: ChannelKnobUnipolar<Unipolar<()>>,
    #[skip_emit]
    #[skip_control]
    flasher: Box<dyn UnsizedFlasher>,
}

impl Default for FlashBang {
    fn default() -> Self {
        Self {
            intensity: Unipolar::new("Intensity", ())
                .at(UnipolarFloat::new(0.1))
                .with_channel_knob(0),
        }
    }
}

impl PatchFixture for FlashBang {
    fn patch_config(options: &mut Options) -> Result<PatchConfig> {
        let double = options
            .remove("double")
            .map(|d| {
                d.parse::<bool>().with_context(|| {
                    format!("invalid \"double\" option \"{d}\"; must be true or false")
                })
            })
            .transpose()?
            .unwrap_or_default();
    }
}

/// Abstract over 5-cell vs 10-cell flashers for different render modes.
trait UnsizedFlasher {
    fn len(&self) -> usize;
    fn cells(&self) -> &[Option<Flash>];
    fn update(&mut self, run: bool, trigger_flash: bool, selected_chase: ChaseIndex, reverse: bool);
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

    fn update(
        &mut self,
        run: bool,
        trigger_flash: bool,
        selected_chase: ChaseIndex,
        reverse: bool,
    ) {
        self.state.update(1);

        let reset = selected_chase != self.selected_chase;
        if reset {
            self.selected_chase = selected_chase;
            self.reset();
        }

        if run && trigger_flash {
            self.flash_next(reverse);
        }
    }
}

impl<const N: usize> Flasher<N> {
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
