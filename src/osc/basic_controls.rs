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
            if v { Some(event_factory()) } else { None }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::{GroupControlMap, MockEmitter, OscClientId};
    use rosc::{OscMessage, OscType};

    #[derive(Debug, PartialEq)]
    enum Msg {
        Triggered,
        State(bool),
        Unipolar(UnipolarFloat),
    }

    fn make_msg(addr: &str, arg: OscType) -> crate::osc::OscControlMessage {
        crate::osc::OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    #[test]
    fn test_button_trigger_fires_on_press() {
        let btn = button("Ctrl");
        let mut map = GroupControlMap::default();
        btn.map_trigger(&mut map, || Msg::Triggered);
        let msg = make_msg("/group/Ctrl", OscType::Float(1.0));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::Triggered);
    }

    #[test]
    fn test_button_trigger_ignores_release() {
        let btn = button("Ctrl");
        let mut map = GroupControlMap::default();
        btn.map_trigger(&mut map, || Msg::Triggered);
        let msg = make_msg("/group/Ctrl", OscType::Float(0.0));
        let result = map.handle(&msg).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_button_state_reports_press() {
        let btn = button("Ctrl");
        let mut map = GroupControlMap::default();
        btn.map_state(&mut map, Msg::State);
        let msg = make_msg("/group/Ctrl", OscType::Float(1.0));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::State(true));
    }

    #[test]
    fn test_button_state_reports_release() {
        let btn = button("Ctrl");
        let mut map = GroupControlMap::default();
        btn.map_state(&mut map, Msg::State);
        let msg = make_msg("/group/Ctrl", OscType::Float(0.0));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::State(false));
    }

    #[test]
    fn test_unipolar_maps_value() {
        let uni = unipolar("Ctrl");
        let mut map = GroupControlMap::default();
        uni.map(&mut map, Msg::Unipolar);
        let msg = make_msg("/group/Ctrl", OscType::Float(0.75));
        let result = map.handle(&msg).unwrap();
        assert_eq!(result.unwrap().0, Msg::Unipolar(UnipolarFloat::new(0.75)));
    }

    #[test]
    fn test_unipolar_rejects_non_float() {
        let uni = unipolar("Ctrl");
        let mut map = GroupControlMap::default();
        uni.map(&mut map, Msg::Unipolar);
        let msg = make_msg("/group/Ctrl", OscType::Int(1));
        assert!(map.handle(&msg).is_err());
    }

    #[test]
    fn test_button_send_true() {
        let btn = button("Ctrl");
        let emitter = MockEmitter::new();
        btn.send(true, &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], ("Ctrl".to_string(), OscType::Float(1.0)));
    }

    #[test]
    fn test_button_send_false() {
        let btn = button("Ctrl");
        let emitter = MockEmitter::new();
        btn.send(false, &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], ("Ctrl".to_string(), OscType::Float(0.0)));
    }

    #[test]
    fn test_unipolar_send_value() {
        let uni = unipolar("Ctrl");
        let emitter = MockEmitter::new();
        uni.send(UnipolarFloat::new(0.5), &emitter);
        let msgs = emitter.take();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0], ("Ctrl".to_string(), OscType::Float(0.5)));
    }
}
