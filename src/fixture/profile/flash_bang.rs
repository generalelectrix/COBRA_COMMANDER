//! Profile for the Monoprice "Flash Bang" 5-ring LED strobe.
//!
//! The profile is designed to use the direct-control 5-channel mode where
//! the brightness of each LED ring is directly controlled. There is also a
//! special 10-channel mode provided for using a pair of these fixtures where
//! the patterns can be extended over both arrays for additional effects.
use anyhow::Result;

use crate::fixture::control::strobe_array::*;
use crate::fixture::prelude::*;

#[derive(EmitState, Control)]
pub struct FlashBang {
    /// Intensity scale.
    ///
    /// These things are BRIGHT - be careful with this control!
    /// An optional max intensity can be set as a group option.
    /// This control is clipped at 1, so a minimum value is "as dim as we can get".
    #[channel_control]
    #[animate]
    intensity: ChannelKnobUnipolar<Unipolar<()>>,
    #[skip_control]
    #[skip_emit]
    max_intensity: u8,
    chase: IndexedSelect<()>,
    multiplier: IndexedSelect<()>,
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
    #[serde(default)]
    max_intensity: Option<u8>,
}

impl PatchFixture for FlashBang {
    const NAME: FixtureType = FixtureType("FlashBang");
    type GroupOptions = GroupOptions;
    type PatchOptions = NoOptions;

    fn new(options: Self::GroupOptions) -> Self {
        let flasher = if options.paired {
            Box::new(Flasher::new(chases_for_paired().unwrap(), false)) as Box<dyn UnsizedFlasher>
        } else {
            Box::new(Flasher::new(chases_for_single().unwrap(), false))
        };

        Self {
            intensity: Unipolar::new("Intensity", ())
                .at(UnipolarFloat::new(0.0))
                .with_channel_knob(0),
            max_intensity: options.max_intensity.unwrap_or(255),
            chase: IndexedSelect::new("Chase", flasher.len(), false, ()),
            multiplier: IndexedSelect::new("Multiplier", 2, false, ()),
            reverse: Bool::new_off("Reverse", ()),
            flasher,
        }
    }

    fn can_strobe() -> Option<StrobeResponse> {
        Some(StrobeResponse::Short)
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
        self.flasher.update(
            update.flash_now,
            self.chase.selected(),
            self.multiplier.selected(),
            self.reverse.val(),
        );
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
            1,
            self.max_intensity,
            self.intensity
                .control
                .val_with_anim(animation_vals.filter(&AnimationTarget::Intensity))
                * group_controls.strobe_clock().intensity(),
        );
        for (flash, chan) in self.flasher.cells().iter().zip(dmx_buf.iter_mut()) {
            *chan = if flash.is_some() { intensity } else { 0 };
            group_controls.preview.intensity_u8(*chan);
        }
    }
}

register_patcher!(FlashBang);

fn chases_for_single() -> Result<Chases<5>> {
    const CELLS: usize = 5;
    let mut c: Chases<CELLS> = Chases::new(&[1])?;

    // single pulse
    c.add_auto_mult(PatternArray::singles(0..CELLS))?;
    // single pulse bounce
    c.add_auto_mult(PatternArray::singles(
        (0..CELLS).chain((1..CELLS - 1).rev()),
    ))?;
    c.add_auto_random();
    c.add_all();
    Ok(c)
}

fn chases_for_paired() -> Result<Chases<10>> {
    const CELLS: usize = 10;
    let mut c: Chases<CELLS> = Chases::new(&[1, 2])?;

    // single pulse
    c.add_auto_mult(PatternArray::singles(0..CELLS))?;

    let bounce_out_in = (0..5).chain((0..4).rev()).chain(5..10).chain((5..9).rev());

    c.add_for_mult(0, PatternArray::singles(bounce_out_in.clone()))?;
    c.add_for_mult(
        1,
        Lockstep::new(
            PatternArray::singles(bounce_out_in.clone()),
            PatternArray::singles(bounce_out_in.clone()),
            9,
        ),
    )?;

    let bounce_in_out = (0..5).rev().chain(1..5).chain((5..10).rev()).chain(6..10);

    c.add_for_mult(0, PatternArray::singles(bounce_in_out.clone()))?;
    c.add_for_mult(
        1,
        Lockstep::new(
            PatternArray::singles(bounce_in_out.clone()),
            PatternArray::singles(bounce_in_out.clone()),
            9,
        ),
    )?;
    // single pulse bounce asymmetric
    c.add_for_mult(
        0,
        PatternArray::singles((0..CELLS).chain((1..CELLS - 1).rev())),
    )?;
    c.add_for_mult(
        1,
        PatternArray::doubles(
            two_flash_spread(CELLS)?.chain(two_flash_spread(CELLS)?.rev().skip(1).take(3)),
        ),
    )?;

    c.add_auto_random();
    // "all" that alternates fixtures
    c.add_for_mult(0, PatternArray::new(vec![[0, 1, 2, 3, 4], [5, 6, 7, 8, 9]]))?;
    c.add_for_mult(1, PatternArray::all())?;
    Ok(c)
}

#[cfg(test)]
mod test {
    #[test]
    fn test_chases() {
        super::chases_for_single().unwrap();
        super::chases_for_paired().unwrap();
    }
}
