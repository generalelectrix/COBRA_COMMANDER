//! Basic control profile for 8-channel auto program control of the Chauvet
//! Freedom Stick.

use super::color::{Color, Model as ColorModel};

use crate::{color::ColorSpace, fixture::prelude::*};

#[derive(Debug, EmitState, Control, Update, PatchFixture)]
#[channel_count = 8]
pub struct FreedomFries {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    color: Color,
    #[channel_control]
    #[animate]
    speed: ChannelKnobUnipolar<UnipolarChannel>,
    strobe: StrobeChannel,
    program: ProgramControl,
}

impl Default for FreedomFries {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 0).with_channel_level(),
            color: Color::for_subcontrol(None, ColorSpace::Hsv),
            speed: Unipolar::full_channel("Speed", 7).with_channel_knob(0),
            strobe: Strobe::channel("Strobe", 5, 11, 255, 0),

            program: ProgramControl::default(),
        }
    }
}

impl AnimatedFixture for FreedomFries {
    type Target = AnimationTarget;
    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.dimmer
            .render(animation_vals.filter(&AnimationTarget::Dimmer), dmx_buf);
        self.speed
            .render(animation_vals.filter(&AnimationTarget::Speed), dmx_buf);
        self.color
            .render_without_animations(ColorModel::Rgb, &mut dmx_buf[1..4]);
        dmx_buf[4] = 0;
        self.strobe
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.program.render(std::iter::empty(), dmx_buf);
    }
}

const PROGRAM_SELECT_LABEL: LabelArray = LabelArray {
    control: "ProgramLabel",
    n: 1,
    empty_label: "",
};

/// Control for indexed program select via a unipolar fader, with
/// value label read-out.
#[derive(Debug)]
struct ProgramControl {
    run_program: Bool<()>,
    select: Unipolar<()>,
    program_cycle_all: Bool<()>,
    selected: usize,
}

impl Default for ProgramControl {
    fn default() -> Self {
        Self {
            run_program: Bool::new_off("RunProgram", ()),
            select: Unipolar::new("Program", ()),
            program_cycle_all: Bool::new_off("ProgramCycleAll", ()),
            selected: 0,
        }
    }
}

impl ProgramControl {
    const PROGRAM_COUNT: usize = 27;
    const DMX_BUF_OFFSET: usize = 6;

    fn render(&self, _animations: impl Iterator<Item = f64>, dmx_buf: &mut [u8]) {
        dmx_buf[Self::DMX_BUF_OFFSET] = if !self.run_program.val() {
            0
        } else if self.program_cycle_all.val() {
            227
        } else {
            ((self.selected * 8) + 11) as u8
        };
    }
}

impl OscControl<()> for ProgramControl {
    fn control_direct(
        &mut self,
        _val: (),
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        bail!("direct control is not implemented for ProgramControl");
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        self.run_program.emit_state(emitter);
        self.select.emit_state(emitter);
        self.program_cycle_all.emit_state(emitter);
        PROGRAM_SELECT_LABEL.set([self.selected.to_string()].into_iter(), emitter);
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.run_program.control(msg, emitter)? {
            return Ok(true);
        }
        if self.program_cycle_all.control(msg, emitter)? {
            return Ok(true);
        }
        if self.select.control(msg, emitter)? {
            let new_val =
                unipolar_to_range(0, Self::PROGRAM_COUNT as u8 - 1, self.select.val()) as usize;
            if new_val >= Self::PROGRAM_COUNT {
                bail!(
                    "program select index {new_val} out of range (max {})",
                    Self::PROGRAM_COUNT
                );
            }
            self.selected =
                unipolar_to_range(0, Self::PROGRAM_COUNT as u8 - 1, self.select.val()) as usize;

            self.select.emit_state(emitter);
            PROGRAM_SELECT_LABEL.set([self.selected.to_string()].into_iter(), emitter);

            return Ok(true);
        }
        Ok(false)
    }
}
