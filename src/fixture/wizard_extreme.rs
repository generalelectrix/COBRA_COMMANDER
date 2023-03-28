//! Martin Wizard Extreme - the one that Goes Slow

use log::error;
use number::{BipolarFloat, UnipolarFloat};

use super::generic::{GenericStrobe, GenericStrobeStateChange};
use super::{EmitFixtureStateChange, Fixture, FixtureControlMessage, PatchFixture};
use crate::master::MasterControls;
use crate::util::{bipolar_to_range, bipolar_to_split_range, unipolar_to_range};
use strum_macros::{Display as EnumDisplay, EnumIter, EnumString};

#[derive(Default, Debug)]
pub struct WizardExtreme {
    dimmer: UnipolarFloat,
    strobe: GenericStrobe,
    color: Color,
    twinkle: bool,
    twinkle_speed: UnipolarFloat,
    gobo: usize,
    drum_rotation: BipolarFloat,
    drum_swivel: BipolarFloat,
    reflector_rotation: BipolarFloat,
}

impl PatchFixture for WizardExtreme {
    fn channel_count(&self) -> usize {
        11
    }
}

impl WizardExtreme {
    const GOBO_COUNT: usize = 14; // includes the open position

    fn handle_state_change(&mut self, sc: StateChange, emitter: &mut dyn EmitFixtureStateChange) {
        use StateChange::*;
        match sc {
            Dimmer(v) => self.dimmer = v,
            Strobe(sc) => self.strobe.handle_state_change(sc),
            Color(c) => self.color = c,
            Twinkle(v) => self.twinkle = v,
            TwinkleSpeed(v) => self.twinkle_speed = v,
            Gobo(v) => {
                if v >= Self::GOBO_COUNT {
                    error!("Gobo select index {} out of range.", v);
                    return;
                }
                self.gobo = v;
            }
            DrumRotation(v) => self.drum_rotation = v,
            DrumSwivel(v) => self.drum_swivel = v,
            ReflectorRotation(v) => self.reflector_rotation = v,
        };
        emitter.emit_wizard_extreme(sc);
    }

    fn render_shutter(&self, master: &MasterControls) -> u8 {
        if self.dimmer == UnipolarFloat::ZERO {
            return 0;
        }
        let strobe_off = 0;
        let strobe = self
            .strobe
            .render_range_with_master(master.strobe(), strobe_off, 189, 130);
        if strobe == strobe_off {
            unipolar_to_range(129, 1, self.dimmer)
        } else {
            strobe
        }
    }
}

impl Fixture for WizardExtreme {
    fn render(&self, master: &MasterControls, dmx_buf: &mut [u8]) {
        dmx_buf[0] = self.render_shutter(master);
        dmx_buf[1] = bipolar_to_split_range(self.reflector_rotation, 2, 63, 127, 66, 0);

        dmx_buf[2] = if self.twinkle {
            // WHY did you put twinkle on the color wheel...
            unipolar_to_range(176, 243, self.twinkle_speed)
        } else {
            self.color.as_dmx()
        };
        dmx_buf[3] = 0; // color shake
        dmx_buf[4] = (self.gobo as u8) * 12;
        dmx_buf[5] = 0; // gobo shake
        dmx_buf[6] = bipolar_to_range(0, 127, self.drum_swivel);
        dmx_buf[7] = bipolar_to_split_range(self.drum_rotation, 2, 63, 127, 66, 0);
        dmx_buf[8] = 0;
        dmx_buf[9] = 0;
        dmx_buf[10] = 0;
    }

    fn emit_state(&self, emitter: &mut dyn EmitFixtureStateChange) {
        use StateChange::*;
        emitter.emit_wizard_extreme(Dimmer(self.dimmer));
        let mut emit_strobe = |ssc| {
            emitter.emit_wizard_extreme(Strobe(ssc));
        };
        self.strobe.emit_state(&mut emit_strobe);
        emitter.emit_wizard_extreme(Color(self.color));
        emitter.emit_wizard_extreme(Twinkle(self.twinkle));
        emitter.emit_wizard_extreme(TwinkleSpeed(self.twinkle_speed));
        emitter.emit_wizard_extreme(Gobo(self.gobo));
        emitter.emit_wizard_extreme(DrumRotation(self.drum_rotation));
        emitter.emit_wizard_extreme(DrumSwivel(self.drum_swivel));
        emitter.emit_wizard_extreme(ReflectorRotation(self.reflector_rotation));
    }

    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn EmitFixtureStateChange,
    ) -> Option<FixtureControlMessage> {
        match msg {
            FixtureControlMessage::WizardExtreme(msg) => {
                self.handle_state_change(msg, emitter);
                None
            }
            other => Some(other),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    Dimmer(UnipolarFloat),
    Strobe(GenericStrobeStateChange),
    Color(Color),
    Twinkle(bool),
    TwinkleSpeed(UnipolarFloat),
    Gobo(usize),
    DrumRotation(BipolarFloat),
    DrumSwivel(BipolarFloat),
    ReflectorRotation(BipolarFloat),
}

pub type ControlMessage = StateChange;

#[derive(Copy, Clone, Debug, Default, PartialEq, EnumString, EnumIter, EnumDisplay)]
pub enum Color {
    #[default]
    Open,
    Blue,
    Orange,
    Purple,
    Green,
    DarkBlue,
    Yellow,
    Magenta,
}

impl Color {
    fn as_dmx(self) -> u8 {
        use Color::*;
        match self {
            Open => 0,
            Blue => 12,
            Orange => 24,
            Purple => 36,
            Green => 48,
            DarkBlue => 60,
            Yellow => 72,
            Magenta => 84,
        }
    }
}

#[derive(Clone, Copy)]
pub enum AnimationTarget {
    Dimmer,
    TwinkleSpeed,
    DrumRotation,
    DrumSwivel,
    ReflectorRotation,
}

impl AnimationTarget {
    /// Return true if this target is unipolar instead of bipolar.
    pub fn is_unipolar(&self) -> bool {
        matches!(self, Self::Dimmer | Self::TwinkleSpeed)
    }
}
