//! A strobing system based on a special-purpose clock/animator pair.
//!
//! This system should work for any fixture that can effectively strobe by
//! modulating the level control with a tuned square wave.  Due to the low DMX
//! framerate of, say, 40-50 Hz, we can't really achieve strobing any better than
//! 10 flashes per second or so. The fact that the physical DMX output is
//! usually unsynchronized with frame writing implies frame tearing, which will
//! also impact the quality of strobing achieved with this system when attempting
//! to hit relatively high strobe rates.
//!
//! The advantage vs. using any given onboard strobe control is that we can
//! easily synchronize the strobing of multiple fixture types across the rig.
use anyhow::Result;
use std::time::Duration;

use number::UnipolarFloat;
use tunnels::{
    clock::{Clock, TapSync},
    transient_indicator::TransientIndicator,
};

use crate::{
    midi::EmitMidiMasterMessage,
    osc::{prelude::*, ScopedControlEmitter},
};

/// Strobe state that subscribers will use to follow the global strobe clock.
/// If active, whatever we fill in for the intensity field should override the
/// channel level when rendering.
#[derive(Default, Debug, Clone, Copy)]
pub struct StrobeState {
    /// If true, strobing behavior should be active.
    pub strobe_on: bool,
    /// Render this intensity.
    pub intensity: UnipolarFloat,
    /// The current strobe rate - this is provided as a potential shim to allow
    /// fixtures that can't be strobed well from DMX to use the legacy strobing
    /// behavior.
    pub rate: UnipolarFloat,
    /// TODO: elimiate this parameter
    pub use_master_rate: bool,
}

pub struct StrobeClock {
    clock: Clock,
    tap_sync: TapSync,
    tick_indicator: TransientIndicator,
    /// The current flash state; if Some, the flash is on.
    /// The value inside represents the number of state updates that the flash
    /// has been active for, so we can decide when to disable it again.
    flash: Option<u8>,
    /// If true, the strobe clock is running.
    strobe_on: bool,
    /// How many frame updates should a flash last for?
    flash_duration: u8,
    /// Intensity of the flash.
    intensity: UnipolarFloat,
    osc_controls: GroupControlMap<ControlMessage>,
}

impl Default for StrobeClock {
    fn default() -> Self {
        let mut osc_controls = GroupControlMap::default();
        map_controls(&mut osc_controls);
        Self {
            clock: Default::default(),
            tap_sync: Default::default(),
            tick_indicator: Default::default(),
            flash: None,
            strobe_on: false,
            flash_duration: 1, // Single-frame flash.
            intensity: UnipolarFloat::ONE,
            osc_controls,
        }
    }
}

impl StrobeClock {
    /// Return the current strobing state.
    pub fn state(&self) -> StrobeState {
        StrobeState {
            strobe_on: self.strobe_on || self.flash.is_some(),
            intensity: self
                .flash
                .is_some()
                .then_some(self.intensity)
                .unwrap_or_default(),
            rate: self.scaled_rate(),
            use_master_rate: true,
        }
    }

    /// Start a flash.
    fn flash(&mut self) {
        self.flash = Some(0);
    }

    pub fn update(
        &mut self,
        delta_t: Duration,
        audio_envelope: UnipolarFloat,
        emitter: &ScopedControlEmitter,
    ) {
        self.clock.update_state(delta_t, audio_envelope);
        // Update the tap sync/rate flasher.
        if let Some(tick_state) = self
            .tick_indicator
            .update_state(delta_t, self.clock.ticked())
        {
            emit_state_change(&StateChange::Ticked(tick_state), emitter);
        }
        // Age the flash if we have one running.
        if let Some(flash_age) = self.flash {
            if flash_age >= self.flash_duration {
                self.flash = None;
            } else {
                self.flash = Some(flash_age + 1);
            }
            println!("flash: {:?}", self.flash);
        }
        // If the strobe clock ticked this frame and we're strobing, flash.
        if self.strobe_on && self.clock.ticked() {
            self.flash();
            println!("ticked flash: {:?}", self.flash);
        }
    }

    fn scaled_rate(&self) -> UnipolarFloat {
        UnipolarFloat::new(self.clock.rate_coarse / RATE_SCALE)
    }

    pub fn emit_state(&self, emitter: &ScopedControlEmitter) {
        use StateChange::*;
        emit_state_change(&Ticked(self.tick_indicator.state()), emitter);
        emit_state_change(&StrobeOn(self.strobe_on), emitter);
        emit_state_change(&Rate(self.scaled_rate()), emitter);
    }

    pub fn control(&mut self, msg: &ControlMessage, emitter: &ScopedControlEmitter) {
        use ControlMessage::*;
        use StateChange::*;
        match msg {
            Set(msg) => self.handle_state_change(msg, emitter),
            Tap => {
                if let Some(new_rate) = self.tap_sync.tap() {
                    self.clock.rate_coarse = new_rate;
                    emit_state_change(&Rate(self.scaled_rate()), emitter);
                }
            }
            ToggleStrobeOn => {
                self.handle_state_change(&StrobeOn(!self.strobe_on), emitter);
            }
            FlashNow => {
                self.flash();
                println!("flash now: {:?}", self.flash);
            }
        }
    }

    pub fn control_osc(
        &mut self,
        msg: &OscControlMessage,
        emitter: &ScopedControlEmitter,
    ) -> Result<()> {
        let Some((msg, _)) = self.osc_controls.handle(msg)? else {
            return Ok(());
        };
        self.control(&msg, emitter);
        Ok(())
    }

    fn handle_state_change(&mut self, msg: &StateChange, emitter: &ScopedControlEmitter) {
        use StateChange::*;
        match *msg {
            StrobeOn(v) => {
                self.strobe_on = v;
                // If we're activating the strobe, make sure we flash immediately.
                if v {
                    self.clock.reset_next_update();
                }
            }
            Rate(v) => self.clock.rate_coarse = v.val() * RATE_SCALE,
            Intensity(v) => self.intensity = v,
            Ticked(_) => (),
        }
        emit_state_change(msg, emitter);
    }
}

fn emit_state_change(sc: &StateChange, emitter: &ScopedControlEmitter) {
    use StateChange::*;
    emitter.emit_midi_strobe_message(sc);
    match *sc {
        Ticked(v) => TAP.send(v, emitter),
        StrobeOn(v) => STROBE_ON.send(v, emitter),
        Rate(v) => RATE.send(v, emitter),
        Intensity(v) => INTENSITY.send(v, emitter),
    }
}

#[derive(Debug, Clone)]
pub enum ControlMessage {
    Set(StateChange),
    Tap,
    ToggleStrobeOn,
    FlashNow,
}

#[derive(Debug, Clone)]
pub enum StateChange {
    /// Outgoing only, no effect as a control.
    Ticked(bool),
    StrobeOn(bool),
    Rate(UnipolarFloat),
    Intensity(UnipolarFloat),
}

/// Knob at full = 10 fps strobing.
const RATE_SCALE: f64 = 10.0;

const FLASH: Button = button("StrobeFlash");
const TAP: Button = button("StrobeTap");
const STROBE_ON: Button = button("StrobeOn");
const RATE: UnipolarOsc = unipolar("StrobeRate");
const INTENSITY: UnipolarOsc = unipolar("StrobeIntensity");

fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    use ControlMessage::*;
    FLASH.map_trigger(map, || FlashNow);
    TAP.map_trigger(map, || Tap);
    STROBE_ON.map_trigger(map, || ToggleStrobeOn);
    RATE.map(map, |v| Set(StateChange::Rate(v)));
    INTENSITY.map(map, |v| Set(StateChange::Intensity(v)))
}

trait EmitMidiStrobeMessage {
    fn emit_midi_strobe_message(&self, msg: &StateChange);
}

impl<T: EmitMidiMasterMessage> EmitMidiStrobeMessage for T {
    fn emit_midi_strobe_message(&self, msg: &StateChange) {
        self.emit_midi_master_message(&crate::master::StateChange::Strobe(msg.clone()));
    }
}
