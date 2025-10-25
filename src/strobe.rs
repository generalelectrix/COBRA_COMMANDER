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
    show::UPDATE_INTERVAL,
};

/// Should a fixture use the short or long flash duration?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrobeResponse {
    Short,
    Long,
}

/// Strobe state that subscribers will use to follow the global strobe clock.
/// If active, whatever we fill in for the intensity field should override the
/// channel level when rendering.
#[derive(Default, Debug, Clone, Copy)]
pub struct StrobeState {
    /// True if the master strobe clock is switched on.
    ///
    /// Note that even when we switch this off, any active flashes will complete
    /// before the intensity override is turned off.
    pub strobe_on: bool,

    /// true if the strobe clock flashed during the last update, either due
    /// to the clock ticking or the manual flash button.
    ///
    /// Fixtures that manage their own internal flash state can use this during
    /// their update method to trigger flashes.
    pub flash_now: bool,

    /// Current value for the mater strobe intensity.
    pub master_intensity: UnipolarFloat,

    /// true if the "short" flash is active.
    short_flash_on: bool,
    /// true if the "long" flash is active.
    long_flash_on: bool,
    /// The current strobe rate - this is provided as a potential shim to allow
    /// fixtures that can't be strobed well from DMX to use the legacy strobing
    /// behavior.
    pub rate: UnipolarFloat,
}

impl StrobeState {
    /// Return Some if strobing is active, with the intensity that should be
    /// rendered.
    ///
    /// Note that this will always be Some if strobing is active, and the
    /// flash state will be encoded in the inner intensity.
    pub fn intensity(&self, response: StrobeResponse) -> Option<UnipolarFloat> {
        let flash_on = match response {
            StrobeResponse::Short => self.short_flash_on,
            StrobeResponse::Long => self.long_flash_on,
        };
        // Return None - as in, no strobe intensity override - if strobe is off
        // and there's no active flash.
        if !self.strobe_on && !flash_on {
            return None;
        }
        // Strobe is on or the relevant flash is active - return an override,
        // either the intensity
        Some(if flash_on {
            self.master_intensity
        } else {
            UnipolarFloat::ZERO
        })
    }
}

pub struct StrobeClock {
    clock: Clock,
    /// This is a rate that is exactly computed from control inputs, such that
    /// we can round-trip this back out to the controls and always get the same
    /// value back out. The actual clock rate set internally is always coerced
    /// to be equal to an integer number of frames, to help get closer to stable
    /// flash timing.
    rate_raw: f64,
    tap_sync: TapSync,
    tick_indicator: TransientIndicator,
    /// The current flash state; if Some, the flash is on.
    /// The value inside represents the number of state updates that the flash
    /// has been active for, so we can decide when to disable it again.
    flash: Option<u8>,
    /// If true, the strobe clock is running.
    strobe_on: bool,
    /// If true, trigger a flash on the next update.
    flash_next_update: bool,
    /// How many frame updates should a short flash last for?
    flash_duration_short: u8,
    /// How many frame updates should a long flash last for?
    flash_duration_long: u8,
    /// Intensity of the flash.
    intensity: UnipolarFloat,
    osc_controls: GroupControlMap<ControlMessage>,
}

impl Default for StrobeClock {
    fn default() -> Self {
        let mut osc_controls = GroupControlMap::default();
        map_controls(&mut osc_controls);

        let mut sc = Self {
            clock: Default::default(),
            rate_raw: 0.,
            tap_sync: Default::default(),
            tick_indicator: Default::default(),
            flash: None,
            strobe_on: false,
            flash_next_update: false,
            flash_duration_short: 1,
            flash_duration_long: 3,
            intensity: UnipolarFloat::ONE,
            osc_controls,
        };
        // Set initial rate to our minimum.
        sc.set_rate(MIN_STROBE_RATE, true);
        sc
    }
}

impl StrobeClock {
    /// Set both raw and coerced rates.
    ///
    /// If coerce is true, discretize the actual rate passed to the clock into
    /// an integer number of frames.
    fn set_rate(&mut self, raw_rate: f64, coerce: bool) {
        self.rate_raw = raw_rate;
        if coerce {
            let interval_frame_count = ((1.0 / raw_rate) / UPDATE_INTERVAL.as_secs_f64()).round();
            let coerced_rate = 1. / (interval_frame_count * UPDATE_INTERVAL.as_secs_f64());
            self.clock.rate_coarse = coerced_rate;
        } else {
            self.clock.rate_coarse = raw_rate;
        }
    }

    /// Update the strobe clock state. Return the updated rendered state.
    pub fn update(
        &mut self,
        delta_t: Duration,
        audio_envelope: UnipolarFloat,
        emitter: &ScopedControlEmitter,
    ) -> StrobeState {
        self.clock.update_state(delta_t, audio_envelope);
        // Update the tap sync/rate flasher.
        if let Some(tick_state) = self
            .tick_indicator
            .update_state(delta_t, self.clock.ticked())
        {
            emit_state_change(&StateChange::Ticked(tick_state), emitter);
        }
        // If the strobe clock ticked this frame and we're strobing, flash.
        // Also flash if we have a queued manual flash.
        let flash_now = (self.strobe_on && self.clock.ticked()) || self.flash_next_update;
        if flash_now {
            self.flash = Some(0);
            self.flash_next_update = false;
        }

        // Age the flash if we have one running.
        let (short_flash_on, long_flash_on) = if let Some(flash_age) = self.flash {
            if flash_age >= self.flash_duration_long {
                self.flash = None;
                (false, false)
            } else {
                let new_age = flash_age + 1;
                self.flash = Some(new_age);
                (new_age <= self.flash_duration_short, true)
            }
        } else {
            (false, false)
        };

        StrobeState {
            strobe_on: self.strobe_on,
            flash_now,
            master_intensity: self.intensity,
            short_flash_on,
            long_flash_on,
            rate: unipolar_from_rate(self.rate_raw),
        }
    }

    pub fn emit_state(&self, emitter: &ScopedControlEmitter) {
        use StateChange::*;
        emit_state_change(&Ticked(self.tick_indicator.state()), emitter);
        emit_state_change(&StrobeOn(self.strobe_on), emitter);
        emit_state_change(&Rate(unipolar_from_rate(self.rate_raw)), emitter);
        emit_state_change(&Intensity(self.intensity), emitter);
    }

    pub fn control(&mut self, msg: &ControlMessage, emitter: &ScopedControlEmitter) {
        use ControlMessage::*;
        use StateChange::*;
        match msg {
            Set(msg) => self.handle_state_change(msg, emitter),
            Tap => {
                if let Some(new_rate) = self.tap_sync.tap() {
                    // Do not coerce tap sync rates to be integer frame numbers;
                    // prefer a bit of jitter but remaining synced with taps
                    // rather than perfect strobing intervals. These are more
                    // likely to be slower anyway, and thus frame jitter will
                    // be less perceptible anyway.
                    self.set_rate(new_rate, false);
                    emit_state_change(&Rate(unipolar_from_rate(new_rate)), emitter);
                }
            }
            ToggleStrobeOn => {
                self.handle_state_change(&StrobeOn(!self.strobe_on), emitter);
            }
            FlashNow => {
                self.flash_next_update = true;
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
            Rate(v) => self.set_rate(rate_from_unipolar(v), true),
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

/// Convert a unipolar control value into a strobe rate.
/// Unlike most clocks, we don't actually want to be able to set a hard 0 for
/// strobe rate, since this would just be confusing.
///
/// Make this a quartic control to more evenly spread out the slow strobe rates.
fn rate_from_unipolar(v: UnipolarFloat) -> f64 {
    MIN_STROBE_RATE + (v.val().powi(4) * (MAX_STROBE_RATE - MIN_STROBE_RATE))
}

/// Convert a strobe rate into a unipolar control parameter.
/// Clamp the incoming value to the expected range to handle values outside of
/// our expected range.
///
/// Take the 4th root to make this a quartic knob.
fn unipolar_from_rate(r: f64) -> UnipolarFloat {
    UnipolarFloat::new(
        ((r.max(MIN_STROBE_RATE) - MIN_STROBE_RATE) / (MAX_STROBE_RATE - MIN_STROBE_RATE))
            .powf(1. / 4.),
    )
}

/// Rate at 0 = strobing once per 2 seconds.
const MIN_STROBE_RATE: f64 = 0.5;

/// Max rate: 40 fps (this is much faster than is useful for strobing single
/// fixtures, but it is as fast as we could possibly strobe cellular fixtures).
const MAX_STROBE_RATE: f64 = 40.;

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
