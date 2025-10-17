use number::UnipolarFloat;

use super::OscControlMessage;

/// An OSC button; can be configured as a stateful button or a trigger/toggle.
#[derive(Clone)]
pub struct Button {
    pub control: &'static str,
}

pub const fn button(control: &'static str) -> Button {
    Button { control }
}

impl Button {
    /// Map a button that creates an event that depends on the button state.
    pub fn map_state<F, T>(&self, map: &mut super::GroupControlMap<T>, process: F)
    where
        F: Fn(bool) -> T + 'static + Copy,
    {
        map.add_fetch_process(self.control, OscControlMessage::get_bool, move |v| {
            Some(process(v))
        })
    }

    /// Map a button that always creates a stateless event when the button is pressed.
    pub fn map_trigger<T>(
        &self,
        map: &mut super::GroupControlMap<T>,
        event_factory: impl Fn() -> T + 'static,
    ) {
        map.add_fetch_process(self.control, OscControlMessage::get_bool, move |v| {
            if v {
                Some(event_factory())
            } else {
                None
            }
        })
    }

    pub fn send<E>(&self, val: bool, emitter: &E)
    where
        E: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        emitter.emit_float(self.control, if val { 1.0 } else { 0.0 });
    }
}

/// An OSC unipolar control.
#[derive(Clone)]
pub struct UnipolarOsc {
    pub control: &'static str,
}

pub const fn unipolar(control: &'static str) -> UnipolarOsc {
    UnipolarOsc { control }
}

impl UnipolarOsc {
    pub fn map<F, T>(&self, map: &mut super::GroupControlMap<T>, process: F)
    where
        F: Fn(UnipolarFloat) -> T + 'static + Copy,
    {
        map.add_fetch_process(self.control, OscControlMessage::get_unipolar, move |v| {
            Some(process(v))
        })
    }

    pub fn send<E>(&self, val: UnipolarFloat, emitter: &E)
    where
        E: crate::osc::EmitScopedOscMessage + ?Sized,
    {
        emitter.emit_float(self.control, val.val());
    }
}
