use std::{collections::VecDeque, time::Duration};

use crate::{fixture::prelude::*, osc::OscControlMessage, util::unipolar_to_range};

#[derive(Debug, EmitState, Control)]
pub struct Comet {
    shutter_open: BoolChannel,
    trigger_state: TriggerState,
    strobe: StrobeChannel,
    shutter_sound_active: BoolChannel,
    macro_pattern: IndexedSelectMenu,
    mirror_speed: UnipolarChannel,
    reset: BoolChannel,
}

impl Default for Comet {
    fn default() -> Self {
        Self {
            shutter_open: Bool::full_channel("Shutter", 0),
            trigger_state: TriggerState::default(),
            // FIXME: need to make strobe rate a quadratic fader
            strobe: Strobe::channel("Strobe", 0, 151, 255, 75),
            shutter_sound_active: Bool::channel("ShutterSoundActive", 0, 0, 125),
            macro_pattern: IndexedSelect::fixed_values("SelectMacro", 1, true, &PATTERN_DMX_VALS),
            mirror_speed: Unipolar::full_channel("Mspeed", 2),
            reset: Bool::full_channel("Reset", 4),
        }
    }
}

const PATTERN_DMX_VALS: [u8; 10] = [12, 35, 65, 85, 112, 140, 165, 190, 212, 240];

impl PatchFixture for Comet {
    const NAME: FixtureType = FixtureType("Comet");
    fn channel_count(&self, _render_mode: Option<RenderMode>) -> usize {
        5
    }
}

crate::register!(Comet);

impl NonAnimatedFixture for Comet {
    fn render(&self, _group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        if !self.shutter_open.val() {
            self.shutter_open.render_no_anim(dmx_buf);
        } else if self.shutter_sound_active.val() {
            self.shutter_sound_active.render_no_anim(dmx_buf);
        } else {
            self.strobe.render_no_anim(dmx_buf);
        }
        self.macro_pattern.render_no_anim(dmx_buf);
        self.mirror_speed.render_no_anim(dmx_buf);
        dmx_buf[3] = self.trigger_state.render();
        self.reset.render_no_anim(dmx_buf);
    }
}
impl ControllableFixture for Comet {
    fn update(&mut self, _: &MasterControls, delta_t: Duration) {
        self.trigger_state.update(delta_t);
    }
}

/// Manage Comet trigger state.
#[derive(Debug)]
struct TriggerState {
    music_trigger: Bool<()>,
    auto_step_rate: Unipolar<()>,
    auto_step: Bool<()>,
    /// queue of step actions to process
    steps_to_take: std::collections::VecDeque<Step>,
    /// what state was this machine in on the last update?
    prior_state: Stepping,
    /// how many frames should we hold current output before re-updating?
    updates_to_hold: usize,
    current_output_value: u8,
}

impl Default for TriggerState {
    fn default() -> Self {
        Self {
            music_trigger: Bool::new_off("TrigSoundActive", ()),
            auto_step_rate: Unipolar::new("AutoStepRate", ()),
            auto_step: Bool::new_off("AutoStep", ()),
            steps_to_take: VecDeque::new(),
            prior_state: Stepping::Idle,
            updates_to_hold: 0,
            current_output_value: Self::DMX_VAL_STOP,
        }
    }
}

impl TriggerState {
    const DMX_VAL_STEP_FORWARD: u8 = 108;
    const DMX_VAL_STEP_BACKWARD: u8 = 142;
    const DMX_VAL_STOP: u8 = 124;
    const DMX_VAL_MUSIC_TRIG: u8 = 50;

    /// rendering to the enttec is asynchronous and frame tearing is a problem
    /// how many updates should we hold the current output before processing another?
    const UPDATES_TO_HOLD: usize = 3;

    fn enqueue_step(&mut self, direction: Step) {
        self.steps_to_take.push_back(direction);
    }

    fn render(&self) -> u8 {
        self.current_output_value
    }

    pub fn update(&mut self, _: Duration) {
        if self.updates_to_hold > 0 {
            self.updates_to_hold -= 1;
        } else {
            self.current_output_value = self._update();
        }
    }

    /// Update the DMX trigger state value.
    ///
    /// This is a fairly complex action, as the step interface at the DMX level
    /// is kinda hokey.
    ///
    /// I give top priority to the mechanism by which we achieve manual stepping.
    /// After that, music responsive mode.  After that, automatic stepping.
    /// The trigger UI should make it clear that music trigger and auto trigger
    /// are mutually exclusive options.
    fn _update(&mut self) -> u8 {
        // what needs to happen to take a step:
        // the dmx value needs to go from its current state to the step value
        // if the current value is the step value, we need to leave and come back again
        if let Some(next_step) = self.steps_to_take.pop_front() {
            // hold this output for a minimum frame count before processing next
            self.updates_to_hold += Self::UPDATES_TO_HOLD - 1;

            if next_step == Step::Forward && self.prior_state != Stepping::Forwards {
                // can take this step, transition to forward
                self.prior_state = Stepping::Forwards;
                return Self::DMX_VAL_STEP_FORWARD;
            } else if next_step == Step::Backward && self.prior_state != Stepping::Backwards {
                // can take this step, transition to backward
                self.prior_state = Stepping::Backwards;
                return Self::DMX_VAL_STEP_BACKWARD;
            } else {
                // we're in the same state as the step we need to take
                // transition to Idle, then take the step on the next update
                self.steps_to_take.push_front(next_step);
                self.prior_state = Stepping::Idle;
                return Self::DMX_VAL_STOP;
            }
        }

        // nothing in the step queue so the state machine is idle
        self.prior_state = Stepping::Idle;

        // if we're not taking a step, easy sauce
        if self.music_trigger.val() {
            Self::DMX_VAL_MUSIC_TRIG
        } else if self.auto_step.val() {
            return unipolar_to_range(151, 255, self.auto_step_rate.val());
        } else {
            return Self::DMX_VAL_STOP;
        }
    }

    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        self.music_trigger.emit_state(emitter);
        self.auto_step_rate.emit_state(emitter);
        self.auto_step.emit_state(emitter);
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        if msg.control() == "StepForwards" {
            self.enqueue_step(Step::Forward);
            return Ok(true);
        }
        if msg.control() == "StepBackwards" {
            self.enqueue_step(Step::Backward);
            return Ok(true);
        }
        if self.music_trigger.control(msg, emitter)? {
            return Ok(true);
        }
        if self.auto_step_rate.control(msg, emitter)? {
            return Ok(true);
        }
        if self.auto_step.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum Stepping {
    Idle,
    Forwards,
    Backwards,
}

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum Step {
    Forward,
    Backward,
}
