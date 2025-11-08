//! Profile for the Big Bar, the American DJ Freq Strobe 16.
//!
//! TODO: merge this and the profile for Flash Bang.
use anyhow::Result;
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
    chase: IndexedSelect<()>,
    multiplier: IndexedSelect<()>,
    reverse: Bool<()>,
    #[skip_emit]
    #[skip_control]
    flasher: Flasher<CELL_COUNT>,
}

impl Default for FreqStrobe {
    fn default() -> Self {
        let flasher = Flasher::new(create_chases().unwrap(), true);
        Self {
            intensity: Unipolar::new("Intensity", ())
                .at_full()
                .with_channel_knob(0),
            chase: IndexedSelect::new("Chase", flasher.len(), false, ()),
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
            self.chase.selected(),
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

fn create_chases() -> Result<Chases<CELL_COUNT>> {
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

#[cfg(test)]
mod test {
    #[test]
    fn test_chases() {
        super::create_chases().unwrap();
    }
}
