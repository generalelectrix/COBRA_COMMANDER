//! Control profle for the Chauvet Swarm 5 FX, aka the Swarmolon.

use log::{debug, error};
use number::UnipolarFloat;

use crate::dmx::DmxAddr;
use crate::fixture::{ControlMessage as ShowControlMessage, EmitStateChange, Fixture};
use crate::generic::{GenericStrobe, GenericStrobeStateChange};
use crate::util::unipolar_to_range;
use strum::IntoEnumIterator;
use strum_macros::{Display as EnumDisplay, EnumIter, EnumString};

pub struct Swarmolon {
    dmx_indices: Vec<usize>,
    derby_color: DerbyColorState,
    derby_strobe: GenericStrobe,
    white_strobe: WhiteStrobe,
}

impl Swarmolon {
    const CHANNEL_COUNT: usize = 9;
    pub fn new(dmx_addrs: Vec<DmxAddr>) -> Self {
        Self {
            dmx_indices: dmx_addrs.iter().map(|a| a - 1).collect(),
            derby_color: DerbyColorState::new(),
            derby_strobe: GenericStrobe::default(),
            white_strobe: WhiteStrobe::default(),
        }
    }

    fn handle_state_change(&mut self, sc: StateChange, emitter: &mut dyn EmitStateChange) {
        use StateChange::*;
        match sc {
            DerbyColor(color, state) => {
                self.derby_color.set(color, state);
            }
            DerbyStrobe(sc) => self.derby_strobe.handle_state_change(sc),
            WhiteStrobe(sc) => {
                if let Err(e) = self.white_strobe.handle_state_change(sc) {
                    error!("{}", e);
                    return;
                }
            }
        };
        emitter.emit_swarmolon(sc);
    }
}

impl Fixture for Swarmolon {
    fn render(&self, dmx_univ: &mut [u8]) {
        for dmx_index in self.dmx_indices.iter() {
            let dmx_slice = &mut dmx_univ[*dmx_index..*dmx_index + Self::CHANNEL_COUNT];
            dmx_slice[0] = 255; // always set to DMX mode
            dmx_slice[1] = self.derby_color.render();
            dmx_slice[3] = if self.derby_strobe.on() {
                unipolar_to_range(5, 254, self.derby_strobe.rate())
            } else {
                0
            };
            debug!("{:?}", dmx_slice);
        }
    }

    fn emit_state(&self, emitter: &mut dyn EmitStateChange) {
        use StateChange::*;
        self.derby_color.emit_state(emitter);
        let mut emit_derby_strobe = |ssc| {
            emitter.emit_swarmolon(DerbyStrobe(ssc));
        };
        self.derby_strobe.emit_state(&mut emit_derby_strobe);
        let mut emit_white_strobe = |ssc| {
            emitter.emit_swarmolon(WhiteStrobe(ssc));
        };
        self.white_strobe.emit_state(&mut emit_white_strobe);
    }

    fn control(
        &mut self,
        msg: ShowControlMessage,
        emitter: &mut dyn EmitStateChange,
    ) -> Option<ShowControlMessage> {
        match msg {
            ShowControlMessage::Swarmolon(msg) => {
                self.handle_state_change(msg, emitter);
                None
            }
            other => Some(other),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    DerbyColor(DerbyColor, bool),
    DerbyStrobe(GenericStrobeStateChange),
    WhiteStrobe(WhiteStrobeStateChange),
}

// No controls that are not represented as state changes.
pub type ControlMessage = StateChange;

#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumString, EnumIter, EnumDisplay, PartialOrd, Ord)]
pub enum DerbyColor {
    Red,
    Green,
    Blue,
    Amber,
    White,
}

struct DerbyColorState(Vec<DerbyColor>);

impl DerbyColorState {
    pub fn new() -> Self {
        Self(Vec::with_capacity(5))
    }

    pub fn set(&mut self, color: DerbyColor, add: bool) {
        if !add {
            self.0.retain(|v| *v != color);
            return;
        }
        if self.0.contains(&color) {
            return;
        }
        self.0.push(color);
        self.0.sort();
    }

    pub fn emit_state(&self, emitter: &mut dyn EmitStateChange) {
        for color in DerbyColor::iter() {
            let state = self.0.contains(&color);
            emitter.emit_swarmolon(StateChange::DerbyColor(color, state));
        }
    }

    pub fn render(&self) -> u8 {
        use DerbyColor::*;
        match self.0[..] {
            [] => 0,
            [Red] => 10,
            [Green] => 15,
            [Blue] => 20,
            [Amber] => 25,
            [White] => 30,
            [Red, White] => 35,
            [Red, Green] => 40,
            [Green, Blue] => 45,
            [Blue, Amber] => 50,
            [Amber, White] => 55,
            [Green, White] => 60,
            [Green, Amber] => 65,
            [Red, Amber] => 70,
            [Red, Blue] => 75,
            [Blue, White] => 80,
            [Red, Green, Blue] => 85,
            [Red, Green, Amber] => 90,
            [Red, Green, White] => 95,
            [Red, Blue, Amber] => 100,
            [Red, Blue, White] => 105,
            [Red, Amber, White] => 110,
            [Green, Blue, Amber] => 115,
            [Green, Blue, White] => 120,
            [Green, Amber, White] => 125,
            [Blue, Amber, White] => 130,
            [Red, Green, Blue, Amber] => 135,
            [Red, Green, Blue, White] => 140,
            [Green, Blue, Amber, White] => 145,
            [Red, Green, Amber, White] => 150,
            [Red, Blue, Amber, White] => 155,
            [Red, Green, Blue, Amber, White] => 160,
            _ => {
                error!("Unmatched derby color state: {:?}.", self.0);
                0
            }
        }
    }
}

#[derive(Debug, Default)]
struct WhiteStrobe {
    state: GenericStrobe,
    /// 0 to 9
    program: usize,
}

impl WhiteStrobe {
    pub fn emit_state<F>(&self, emit: &mut F)
    where
        F: FnMut(WhiteStrobeStateChange),
    {
        use WhiteStrobeStateChange::*;
        emit(Program(self.program));
        let mut emit_general = |gsc| {
            emit(State(gsc));
        };
        self.state.emit_state(&mut emit_general);
    }

    pub fn handle_state_change(&mut self, sc: WhiteStrobeStateChange) -> Result<(), String> {
        use WhiteStrobeStateChange::*;
        match sc {
            State(g) => self.state.handle_state_change(g),
            Program(p) => {
                if p > 9 {
                    return Err(format!(
                        "swarmolon white strobe program index out of range: {}",
                        p
                    ));
                }
                self.program = p
            }
        }
        Ok(())
    }

    pub fn render(&self) -> u8 {
        if !self.state.on() {
            return 0;
        }
        let program_base = (self.program + 1) * 10;
        let program_speed = unipolar_to_range(0, 9, self.state.rate());
        program_base as u8 + program_speed
    }
}

#[derive(Clone, Copy, Debug)]
pub enum WhiteStrobeStateChange {
    /// Valid range is 0 to 9.
    Program(usize),
    State(GenericStrobeStateChange),
}
