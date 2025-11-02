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
use anyhow::{anyhow, Result};
use std::time::Duration;
use strum::VariantArray;
use strum_macros::VariantArray;

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

/// A strobe flash; when on, lasts for an integer number of frames.
#[derive(Debug)]
pub struct FlashState {
    duration: u8,
    flash: Option<u8>,
}

impl FlashState {
    pub fn new(response: StrobeResponse) -> Self {
        Self {
            duration: response.length(),
            flash: None,
        }
    }

    /// Start a new flash.
    pub fn flash_now(&mut self) {
        self.flash = Some(self.duration);
    }

    /// Return true if this flash is on.
    pub fn is_on(&self) -> bool {
        self.flash.is_some()
    }

    /// Update this flash by the provided number of frames.
    pub fn update(&mut self, n_frames: u8) {
        let Some(age) = self.flash else {
            return;
        };
        let age = age.saturating_add(n_frames);
        if age >= self.duration {
            self.flash = None;
        } else {
            self.flash = Some(age);
        }
    }
}

/// Should a fixture use the short or long flash duration?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrobeResponse {
    Short,
    Long,
}

impl StrobeResponse {
    pub fn length(self) -> u8 {
        match self {
            Self::Short => 1,
            Self::Long => 3,
        }
    }
}

#[derive(Debug)]
pub struct Distributor {
    request_count: usize,
    strategy: Option<DistributorStrategy>,
}

impl Distributor {
    pub fn flash_now(&mut self, strobe_active_for_group: bool) -> bool {
        use DistributorStrategy::*;
        if !strobe_active_for_group {
            return false;
        }
        let flash_now = match self.strategy {
            None => false,
            Some(All) => true,
            Some(One(i)) => i == self.request_count,
        };
        self.request_count += 1;
        flash_now
    }
}

/// Distribute flashes.
#[derive(Debug)]
enum DistributorStrategy {
    /// Strobe every group at once.
    All,
    /// Strobe one group index.
    One(usize),
}

/// Keep track of flash distribution state.
enum FlashDistribution {
    /// Strobe every group at once.
    All,
    /// Strobe this group index next.
    One(usize),
}

impl FlashDistribution {
    /// Advance flash distribution for the next flash.
    /// Take the number of active strobe targets into account.
    pub fn advance(&mut self, group_count: usize) {
        if group_count == 0 {
            return;
        }
        match self {
            Self::All => (),
            Self::One(i) => {
                *i = (*i + 1) % group_count;
            }
        }
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
    /// Multiply the clock rate by this value.
    rate_mult: Multiplier,
    tap_sync: TapSync,
    /// True if the current rate was set from a tap.
    set_from_tap: bool,
    tick_indicator: TransientIndicator,
    /// If true, the strobe clock is running.
    strobe_on: bool,
    /// If true, trigger a flash on the next update.
    flash_next_update: bool,
    /// If true, the strobe clock ticked on this update.
    flash_now: bool,
    /// Intensity of the flash.
    intensity: UnipolarFloat,
    /// Current flash distribution strategy.
    distribution: FlashDistribution,
    osc_controls: GroupControlMap<ControlMessage>,
}

impl Default for StrobeClock {
    fn default() -> Self {
        let mut osc_controls = GroupControlMap::default();
        map_controls(&mut osc_controls);

        let mut sc = Self {
            clock: Default::default(),
            rate_raw: MIN_STROBE_RATE,
            rate_mult: Default::default(),
            tap_sync: Default::default(),
            set_from_tap: false,
            tick_indicator: Default::default(),
            strobe_on: false,
            flash_next_update: false,
            flash_now: false,
            intensity: UnipolarFloat::ONE,
            distribution: FlashDistribution::All,
            osc_controls,
        };
        // Set initial rate to our minimum.
        sc.apply_rate();
        sc
    }
}

impl StrobeClock {
    /// Set the actual rate used by the inner clock.
    ///
    /// Discretize the actual rate passed to the clock into
    /// an integer number of frames when the rate has been set from a slider.
    ///
    /// Apply clock multipliers at this step.
    fn apply_rate(&mut self) {
        let rate = self.rate_raw * self.rate_mult.as_f64();
        self.clock.rate_coarse = if self.set_from_tap {
            // Do not coerce tap sync rates to be integer frame numbers;
            // prefer a bit of jitter but remaining synced with taps
            // rather than perfect strobing intervals. These are more
            // likely to be slower anyway, and thus frame jitter will
            // be less perceptible anyway.
            rate
        } else {
            let interval_frame_count = ((1.0 / rate) / UPDATE_INTERVAL.as_secs_f64()).round();
            1. / (interval_frame_count * UPDATE_INTERVAL.as_secs_f64())
        };
    }

    /// Update the strobe clock state. Return the updated rendered state.
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
        // If the strobe clock ticked this frame and we're strobing, flash.
        // Also flash if we have a queued manual flash.
        self.flash_now = (self.strobe_on && self.clock.ticked()) || self.flash_next_update;
        if self.flash_now {
            self.flash_next_update = false;
        }
    }

    /// Return the current set intensity.
    pub fn intensity(&self) -> UnipolarFloat {
        self.intensity
    }

    /// Return true if the master strobe enable is turned on.
    pub fn strobe_on(&self) -> bool {
        self.strobe_on
    }

    /// Get the current rate, rescaled back into a unipolar control.
    pub fn rate_control(&self) -> UnipolarFloat {
        unipolar_from_rate(self.rate_raw)
    }

    /// Get a flash distributor if we have a flash this frame.
    /// Potentially update the state of the distributor if the number of
    /// strobed groups isn't compatible with current settings.
    pub fn distributor(&mut self, group_count: usize) -> Distributor {
        if !self.flash_now || group_count == 0 {
            return Distributor {
                request_count: 0,
                strategy: None,
            };
        }
        self.distribution.advance(group_count);
        Distributor {
            request_count: 0,
            strategy: match self.distribution {
                FlashDistribution::All => Some(DistributorStrategy::All),
                FlashDistribution::One(i) => Some(DistributorStrategy::One(i)),
            },
        }
    }

    pub fn emit_state(&self, emitter: &ScopedControlEmitter) {
        use StateChange::*;
        emit_state_change(&Ticked(self.tick_indicator.state()), emitter);
        emit_state_change(&StrobeOn(self.strobe_on), emitter);
        emit_state_change(&Rate(unipolar_from_rate(self.rate_raw)), emitter);
        emit_state_change(&Intensity(self.intensity), emitter);
        emit_state_change(&Mult(self.rate_mult), emitter);
    }

    pub fn control(&mut self, msg: &ControlMessage, emitter: &ScopedControlEmitter) {
        use ControlMessage::*;
        use StateChange::*;
        match msg {
            Set(msg) => self.handle_state_change(msg, emitter),
            Tap => {
                if let Some(new_rate) = self.tap_sync.tap() {
                    self.set_from_tap = true;
                    self.rate_raw = new_rate;
                    self.apply_rate();
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
            Rate(v) => {
                self.rate_raw = rate_from_unipolar(v);
                self.set_from_tap = false;
                self.apply_rate();
            }
            Intensity(v) => self.intensity = v,
            Ticked(_) => (),
            Mult(m) => {
                self.rate_mult = m;
                self.apply_rate();
            }
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
        Mult(m) => MULT.set(m.as_index(), false, emitter),
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
    Mult(Multiplier),
}

/// Apply this multiplier to the strobe clock.
#[derive(Default, Debug, Clone, Copy, VariantArray)]
pub enum Multiplier {
    #[default]
    One,
    Two,
    Three,
    Four,
    Eight,
}

impl Multiplier {
    pub fn as_f64(self) -> f64 {
        match self {
            Self::One => 1.,
            Self::Two => 2.,
            Self::Three => 3.,
            Self::Four => 4.,
            Self::Eight => 8.,
        }
    }

    pub fn as_index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
            Self::Three => 2,
            Self::Four => 3,
            Self::Eight => 4,
        }
    }
}

const FLASH: Button = button("StrobeFlash");
const TAP: Button = button("StrobeTap");
const STROBE_ON: Button = button("StrobeOn");
const RATE: UnipolarOsc = unipolar("StrobeRate");
const INTENSITY: UnipolarOsc = unipolar("StrobeIntensity");
const MULT: RadioButton = RadioButton {
    control: "StrobeMult",
    n: 5,
    x_primary_coordinate: false,
};

fn map_controls(map: &mut GroupControlMap<ControlMessage>) {
    use ControlMessage::*;
    FLASH.map_trigger(map, || FlashNow);
    TAP.map_trigger(map, || Tap);
    STROBE_ON.map_trigger(map, || ToggleStrobeOn);
    RATE.map(map, |v| Set(StateChange::Rate(v)));
    INTENSITY.map(map, |v| Set(StateChange::Intensity(v)));
    MULT.map_fallible(map, |i| {
        Ok(ControlMessage::Set(StateChange::Mult(
            Multiplier::VARIANTS
                .get(i)
                .copied()
                .ok_or_else(|| anyhow!("strobe multiplier index {i} out of range"))?,
        )))
    });
}

trait EmitMidiStrobeMessage {
    fn emit_midi_strobe_message(&self, msg: &StateChange);
}

impl<T: EmitMidiMasterMessage> EmitMidiStrobeMessage for T {
    fn emit_midi_strobe_message(&self, msg: &StateChange) {
        self.emit_midi_master_message(&crate::master::StateChange::Strobe(msg.clone()));
    }
}
