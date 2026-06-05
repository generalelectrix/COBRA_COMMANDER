//! Per-fixture position offsets for moving heads.
//!
//! Each positionable [`crate::fixture::FixtureGroup`] owns a [`Positioner`]
//! that stores per-fixture `(x, y, focus)` offsets across 8 named preset
//! slots, plus the channel-scoped editing state (selected fixture, bump
//! step) needed to drive the operator's editing UI.

use anyhow::{Result, bail};
use number::BipolarFloat;
use rosc::OscType;

use crate::osc::prelude::RadioButton;
use crate::osc::{
    EmitScopedOscMessage, FixtureStateEmitter, OscControlMessage, ScopedOscMessage,
    positioner as addr,
};

/// Number of preset slots per positionable group.
pub const N_POSITIONER_SLOTS: usize = 8;

/// Maximum number of axes a single fixture can contribute (x, y, optional
/// focus). Used to size the stack buffer in `FixtureWithAnimations::render`.
pub const N_POSITIONER_AXES: usize = 3;

/// Per-group positioner state: preset slots, the currently-active slot, and
/// channel-scoped editing state (selected fixture, bump granularity).
#[derive(Debug)]
pub struct Positioner {
    pub presets: [PositionPreset; N_POSITIONER_SLOTS],
    /// Active preset slot (`0..N_POSITIONER_SLOTS`).
    pub active: usize,
    /// Index of the fixture being edited via the channel-scoped Positioner
    /// tab (`0..fixture_count`).
    pub selected_fixture: usize,
    /// Step magnitude for the channel-scoped bump buttons.
    pub bump_step: BumpStep,
    /// Number of fixtures this positioner is sized for; always equals every
    /// preset's `offsets.len()`.
    fixture_count: usize,
}

/// One preset slot's data: a name and a per-fixture offset vector.
#[derive(Debug, Clone)]
pub struct PositionPreset {
    /// Always populated. Defaults to `"Position {1..8}"` until the operator
    /// renames it via the desktop GUI.
    pub name: String,
    /// One entry per fixture in the group; `offsets[i]` is the offset for
    /// fixture index `i`. Reconciled at repatch time.
    pub offsets: Vec<PositionOffset>,
}

/// Per-fixture offset along the positioner's axes. The `focus` value is
/// stored uniformly across all positionable groups but only contributes to
/// render when the fixture type's [`PositionerAxes::focus`] is `Some`.
#[derive(Debug, Default, Clone, Copy)]
pub struct PositionOffset {
    pub x: BipolarFloat,
    pub y: BipolarFloat,
    pub focus: BipolarFloat,
}

/// Maps the positioner's logical axes (`x`, `y`, optional `focus`) to the
/// concrete animation target enum variants for a specific fixture type.
/// Declared by [`crate::fixture::AnimatedFixture::positioner_axes`] when a
/// fixture opts into the positioner.
#[derive(Debug, Clone, Copy)]
pub struct PositionerAxes<T> {
    pub x: T,
    pub y: T,
    /// `None` for fixtures without a focus parameter (e.g. moving-head LED
    /// washes like the iWashLed). When `None`, the focus offset is still
    /// stored but never contributes to DMX.
    pub focus: Option<T>,
}

/// Step magnitude for the channel-scoped bump buttons. Same step applies to
/// X, Y, and Focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpStep {
    /// ~0.05; broad, fast positioning.
    Coarse,
    /// ~0.01; default; comfortable for soundcheck adjustments.
    Medium,
    /// ~0.002; precision tweaks. Still well above the 16-bit LSB of ~3e-5.
    Fine,
}

impl BumpStep {
    /// The bipolar-range delta applied per bump press.
    pub fn magnitude(&self) -> f64 {
        match self {
            Self::Coarse => 0.05,
            Self::Medium => 0.01,
            Self::Fine => 0.002,
        }
    }
}

/// Which positioner axis a control message addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Focus,
}

/// Sign of a bump delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    Plus,
    Minus,
}

impl Positioner {
    /// Build a fresh positioner for a group with `fixture_count` fixtures.
    ///
    /// All 8 preset slots are initialized with the default name (`"Position
    /// 1"` through `"Position 8"`) and `fixture_count` zeroed offsets each.
    /// `active = 0`, `selected_fixture = 0`, `bump_step = Medium`.
    pub fn default_for(fixture_count: usize) -> Self {
        let presets = std::array::from_fn(|i| PositionPreset {
            name: format!("Position {}", i + 1),
            offsets: vec![PositionOffset::default(); fixture_count],
        });
        Self {
            presets,
            active: 0,
            selected_fixture: 0,
            bump_step: BumpStep::Medium,
            fixture_count,
        }
    }

    /// Grow or shrink each preset's `offsets` vector to match a new fixture
    /// count, preserving existing values where they overlap. Used during
    /// repatch reconciliation when a positionable group gains or loses
    /// fixtures.
    ///
    /// Extending pads with default (zero) offsets; truncating drops the tail
    /// entries.
    pub fn reconcile_to_fixture_count(&mut self, new_count: usize) {
        for preset in &mut self.presets {
            preset
                .offsets
                .resize_with(new_count, PositionOffset::default);
        }
        self.fixture_count = new_count;
        // If a shrink dropped the previously-selected fixture, clamp.
        if self.selected_fixture >= new_count {
            self.selected_fixture = new_count.saturating_sub(1);
        }
    }
}

/// What kind of state mutation a `control_osc` dispatch produced, governing
/// which emit paths fire afterward.
enum Mutation {
    /// `active` changed: both per-group and channel-scoped views are stale.
    ActiveChanged,
    /// Some other state changed (offset, bump step, selected fixture).
    /// Only the channel-scoped view is stale.
    Other,
}

impl Positioner {
    /// Handle a per-group positioner OSC message (`PositionPresetSelect`).
    /// Returns `None` for any other address (signaling fall-through),
    /// `Some(Ok(()))` on a successful handle, `Some(Err(_))` for a
    /// recognized-but-malformed message.
    pub fn control_osc_per_group(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Option<Result<()>> {
        let result = match msg.control() {
            c if c == addr::POSITION_PRESET_SELECT.control => {
                self.handle_preset_select(msg, &addr::POSITION_PRESET_SELECT)
            }
            _ => return None,
        };
        Some(self.finish(result, emitter))
    }

    /// Handle a channel-scoped positioner OSC message (X/Y/Focus faders and
    /// bumps, BumpStep, Prev/Next, Preset, Reset, ResetPreset). Returns
    /// `Err` for an unrecognized address or a recognized-but-malformed
    /// message.
    pub fn control_osc_channel_scoped(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        let result = match msg.control() {
            addr::X_FADER => self.handle_fader(msg, Axis::X),
            addr::Y_FADER => self.handle_fader(msg, Axis::Y),
            addr::FOCUS_FADER => self.handle_fader(msg, Axis::Focus),

            addr::X_BUMP_UP => self.handle_bump(msg, Axis::X, Sign::Plus),
            addr::X_BUMP_DOWN => self.handle_bump(msg, Axis::X, Sign::Minus),
            addr::Y_BUMP_UP => self.handle_bump(msg, Axis::Y, Sign::Plus),
            addr::Y_BUMP_DOWN => self.handle_bump(msg, Axis::Y, Sign::Minus),
            addr::FOCUS_BUMP_UP => self.handle_bump(msg, Axis::Focus, Sign::Plus),
            addr::FOCUS_BUMP_DOWN => self.handle_bump(msg, Axis::Focus, Sign::Minus),

            c if c == addr::BUMP_STEP_SELECT.control => self.handle_bump_step_select(msg),

            addr::PREV_FIXTURE => self.handle_nudge_fixture(msg, Sign::Minus),
            addr::NEXT_FIXTURE => self.handle_nudge_fixture(msg, Sign::Plus),

            c if c == addr::PRESET_SELECT.control => {
                self.handle_preset_select(msg, &addr::PRESET_SELECT)
            }

            addr::RESET_FIXTURE => self.handle_reset_fixture(msg),
            addr::RESET_PRESET => self.handle_reset_preset(msg),

            other => bail!("unrecognized channel-scoped positioner control: {other}"),
        };
        self.finish(result, emitter)
    }

    /// Fire the emit paths appropriate to `mutation` and the emitter's
    /// channel binding. `None` means the handler produced no observable
    /// mutation (button release, no-op tap, out-of-range index) — skip
    /// the emit entirely.
    fn finish(
        &self,
        result: Result<Option<Mutation>>,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        let Some(mutation) = result? else {
            return Ok(());
        };
        if matches!(mutation, Mutation::ActiveChanged) {
            self.emit_per_group_state(emitter);
        }
        if emitter.channel().is_current() {
            let channel_emitter = emitter.scoped(addr::GROUP);
            self.emit_channel_state(&channel_emitter);
        }
        Ok(())
    }

    fn handle_fader(&mut self, msg: &OscControlMessage, axis: Axis) -> Result<Option<Mutation>> {
        let val = msg.get_bipolar()?;
        if let Some(offset) = self
            .presets
            .get_mut(self.active)
            .and_then(|preset| preset.offsets.get_mut(self.selected_fixture))
        {
            match axis {
                Axis::X => offset.x = val,
                Axis::Y => offset.y = val,
                Axis::Focus => offset.focus = val,
            }
        }
        Ok(Some(Mutation::Other))
    }

    fn handle_bump(
        &mut self,
        msg: &OscControlMessage,
        axis: Axis,
        sign: Sign,
    ) -> Result<Option<Mutation>> {
        if !msg.get_bool()? {
            return Ok(None);
        }
        let signed_delta = match sign {
            Sign::Plus => self.bump_step.magnitude(),
            Sign::Minus => -self.bump_step.magnitude(),
        };
        if let Some(offset) = self
            .presets
            .get_mut(self.active)
            .and_then(|preset| preset.offsets.get_mut(self.selected_fixture))
        {
            match axis {
                Axis::X => offset.x = BipolarFloat::new(offset.x.val() + signed_delta),
                Axis::Y => offset.y = BipolarFloat::new(offset.y.val() + signed_delta),
                Axis::Focus => offset.focus = BipolarFloat::new(offset.focus.val() + signed_delta),
            }
        }
        Ok(Some(Mutation::Other))
    }

    fn handle_bump_step_select(&mut self, msg: &OscControlMessage) -> Result<Option<Mutation>> {
        let Some(index) = addr::BUMP_STEP_SELECT.parse_press(msg)? else {
            return Ok(None);
        };
        self.bump_step = match index {
            0 => BumpStep::Coarse,
            1 => BumpStep::Medium,
            2 => BumpStep::Fine,
            _ => return Ok(None),
        };
        Ok(Some(Mutation::Other))
    }

    fn handle_nudge_fixture(
        &mut self,
        msg: &OscControlMessage,
        sign: Sign,
    ) -> Result<Option<Mutation>> {
        if !msg.get_bool()? || self.fixture_count == 0 {
            return Ok(None);
        }
        let delta: isize = match sign {
            Sign::Plus => 1,
            Sign::Minus => -1,
        };
        let new = (self.selected_fixture as isize + delta).rem_euclid(self.fixture_count as isize);
        self.selected_fixture = new as usize;
        Ok(Some(Mutation::Other))
    }

    fn handle_preset_select(
        &mut self,
        msg: &OscControlMessage,
        primitive: &RadioButton,
    ) -> Result<Option<Mutation>> {
        let Some(index) = primitive.parse_press(msg)? else {
            return Ok(None);
        };
        if index >= N_POSITIONER_SLOTS || self.active == index {
            return Ok(None);
        }
        self.active = index;
        Ok(Some(Mutation::ActiveChanged))
    }

    fn handle_reset_fixture(&mut self, msg: &OscControlMessage) -> Result<Option<Mutation>> {
        if !msg.get_bool()? {
            return Ok(None);
        }
        if let Some(offset) = self
            .presets
            .get_mut(self.active)
            .and_then(|preset| preset.offsets.get_mut(self.selected_fixture))
        {
            *offset = PositionOffset::default();
        }
        Ok(Some(Mutation::Other))
    }

    fn handle_reset_preset(&mut self, msg: &OscControlMessage) -> Result<Option<Mutation>> {
        if !msg.get_bool()? {
            return Ok(None);
        }
        if let Some(preset) = self.presets.get_mut(self.active) {
            for off in &mut preset.offsets {
                *off = PositionOffset::default();
            }
        }
        Ok(Some(Mutation::Other))
    }

    /// Push the channel-scoped Positioner tab state. The emitter should be
    /// scoped to the [`addr::GROUP`] entity.
    pub fn emit_channel_state<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        let label = if self.fixture_count == 0 {
            "—".to_string()
        } else {
            format!("{} / {}", self.selected_fixture + 1, self.fixture_count)
        };
        emitter.emit_osc(ScopedOscMessage {
            control: addr::FIXTURE_LABEL,
            arg: OscType::String(label),
        });

        let (x, y, focus) = match self
            .presets
            .get(self.active)
            .and_then(|preset| preset.offsets.get(self.selected_fixture))
        {
            Some(off) => (off.x.val(), off.y.val(), off.focus.val()),
            None => (0.0, 0.0, 0.0),
        };
        emitter.emit_float(addr::X_FADER, x);
        emitter.emit_float(addr::Y_FADER, y);
        emitter.emit_float(addr::FOCUS_FADER, focus);

        addr::PRESET_SELECT.set(self.active, false, emitter);
        addr::PRESET_LABELS.set(self.presets.iter().map(|p| p.name.clone()), emitter);

        let bump_index = match self.bump_step {
            BumpStep::Coarse => 0,
            BumpStep::Medium => 1,
            BumpStep::Fine => 2,
        };
        addr::BUMP_STEP_SELECT.set(bump_index, false, emitter);
    }

    /// Push the per-group preset selector state (radio index + 8 labels).
    /// The emitter should be scoped to the group's name (e.g. via the
    /// [`FixtureStateEmitter`] that prefixes addresses with the group name).
    pub fn emit_per_group_state<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        addr::POSITION_PRESET_SELECT.set(self.active, false, emitter);
        addr::POSITION_PRESET_LABEL.set(self.presets.iter().map(|p| p.name.clone()), emitter);
    }
}

/// Push neutral / cleared values for every channel-scoped Positioner control.
///
/// Used in two cases where there is no live positioner to drive the
/// `/Positioner/...` tab:
///
/// - The current channel's group has no positioner (non-positionable
///   fixture type).
/// - There is no current channel at all (e.g. an empty patch at cold
///   start).
///
/// Without this emit, the tab would display stale state lingering from
/// whichever positionable channel was last selected, which can mislead the
/// operator into thinking they're still editing it. The plan specifies the
/// FixtureLabel `"—"` as the visible cue; we also reset the faders and
/// deselect both radios so the tab visually matches the "no live state"
/// condition.
///
/// The emitter should be scoped to [`addr::GROUP`] (`"Positioner"`).
pub fn emit_non_positionable_channel_state<E: EmitScopedOscMessage + ?Sized>(emitter: &E) {
    emitter.emit_osc(ScopedOscMessage {
        control: addr::FIXTURE_LABEL,
        arg: OscType::String("—".to_string()),
    });
    emitter.emit_float(addr::X_FADER, 0.0);
    emitter.emit_float(addr::Y_FADER, 0.0);
    emitter.emit_float(addr::FOCUS_FADER, 0.0);
    // Deselect every preset / bump-step button via an out-of-range set.
    addr::PRESET_SELECT.set(usize::MAX, /* allow_out_of_range = */ true, emitter);
    // Clear all 8 preset labels (LabelArray fills empty slots with the
    // configured empty_label, which is "" — TouchOSC then shows blanks).
    addr::PRESET_LABELS.set(std::iter::empty(), emitter);
    addr::BUMP_STEP_SELECT.set(usize::MAX, true, emitter);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_for_seeds_named_presets_and_zero_offsets() {
        let p = Positioner::default_for(4);
        assert_eq!(p.active, 0);
        assert_eq!(p.selected_fixture, 0);
        assert_eq!(p.bump_step, BumpStep::Medium);
        for (i, preset) in p.presets.iter().enumerate() {
            assert_eq!(preset.name, format!("Position {}", i + 1));
            assert_eq!(preset.offsets.len(), 4);
            for off in &preset.offsets {
                assert_eq!(off.x.val(), 0.0);
                assert_eq!(off.y.val(), 0.0);
                assert_eq!(off.focus.val(), 0.0);
            }
        }
    }

    #[test]
    fn reconcile_grows_with_zero_offsets() {
        let mut p = Positioner::default_for(2);
        // Set non-default values in slot 0 to verify they survive.
        p.presets[0].offsets[0].x = BipolarFloat::new(0.5);
        p.presets[0].offsets[1].y = BipolarFloat::new(-0.25);
        p.reconcile_to_fixture_count(5);
        assert_eq!(p.presets[0].offsets.len(), 5);
        assert_eq!(p.presets[0].offsets[0].x.val(), 0.5);
        assert_eq!(p.presets[0].offsets[1].y.val(), -0.25);
        for off in &p.presets[0].offsets[2..] {
            assert_eq!(off.x.val(), 0.0);
            assert_eq!(off.y.val(), 0.0);
            assert_eq!(off.focus.val(), 0.0);
        }
    }

    #[test]
    fn reconcile_shrinks_by_truncation_and_clamps_selected_fixture() {
        let mut p = Positioner::default_for(5);
        p.selected_fixture = 4;
        p.presets[0].offsets[2].x = BipolarFloat::new(0.75);
        p.reconcile_to_fixture_count(3);
        assert_eq!(p.presets[0].offsets.len(), 3);
        assert_eq!(p.presets[0].offsets[2].x.val(), 0.75);
        // selected_fixture was 4, but now max is 2.
        assert_eq!(p.selected_fixture, 2);
    }

    #[test]
    fn reconcile_to_zero_clamps_selected_fixture_to_zero() {
        let mut p = Positioner::default_for(3);
        p.selected_fixture = 2;
        p.reconcile_to_fixture_count(0);
        assert_eq!(p.presets[0].offsets.len(), 0);
        assert_eq!(p.selected_fixture, 0);
    }

    #[test]
    fn bump_step_magnitudes() {
        assert!((BumpStep::Coarse.magnitude() - 0.05).abs() < 1e-9);
        assert!((BumpStep::Medium.magnitude() - 0.01).abs() < 1e-9);
        assert!((BumpStep::Fine.magnitude() - 0.002).abs() < 1e-9);
    }

    fn make_msg(addr: &str, arg: OscType) -> OscControlMessage {
        OscControlMessage::new(
            rosc::OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            crate::osc::OscClientId::example(),
        )
        .unwrap()
    }

    /// A `FixtureStateEmitter` that drops everything emitted through it.
    fn null_fixture_emitter<'a>(
        name: &'a crate::config::GroupName,
    ) -> crate::osc::FixtureStateEmitter<'a> {
        crate::osc::FixtureStateEmitter::new(name, crate::channel::mock::no_op_emitter())
    }

    /// Regression: per-group dispatch must NOT match channel-scoped
    /// vocabulary. Otherwise a fixture's own `/MyFixture/Focus` (or
    /// `/MyFixture/Reset`, `/MyFixture/X`, etc.) would be swallowed by
    /// the positioner before reaching the fixture's own control handler.
    #[test]
    fn control_osc_per_group_ignores_channel_scoped_and_unknown_addresses() {
        let mut p = Positioner::default_for(4);
        let name = crate::config::GroupName("MyFixture".to_string());
        let emitter = null_fixture_emitter(&name);

        // Channel-scoped controls that a real fixture might collide with.
        for ctrl in [
            "X",
            "Y",
            "Focus",
            "Reset",
            "ResetPreset",
            "Prev",
            "Next",
            "Preset",
            "BumpStep",
            "XBumpUp",
            "FocusBumpDown",
        ] {
            let msg = make_msg(&format!("/MyFixture/{ctrl}"), OscType::Float(1.0));
            let result = p.control_osc_per_group(&msg, &emitter);
            assert!(
                result.is_none(),
                "/{name}/{ctrl} must fall through to the fixture, but per-group dispatch matched it: {result:?}",
                name = "MyFixture",
            );
        }

        // Arbitrary fixture-specific controls.
        for ctrl in ["Hue", "Sat", "Pan", "Tilt", "SomethingElse"] {
            let msg = make_msg(&format!("/MyFixture/{ctrl}"), OscType::Float(1.0));
            assert!(p.control_osc_per_group(&msg, &emitter).is_none());
        }
    }

    #[test]
    fn control_osc_channel_scoped_errors_on_unknown_address() {
        let mut p = Positioner::default_for(1);
        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);

        let msg = make_msg("/Positioner/NotAControl", OscType::Float(1.0));
        let err = p
            .control_osc_channel_scoped(&msg, &emitter)
            .expect_err("unknown control should error");
        assert!(
            err.to_string().contains("unrecognized"),
            "error didn't mention unrecognized: {err}",
        );
    }

    #[test]
    fn bump_up_clamps_at_positive_one() {
        let mut p = Positioner::default_for(1);
        p.bump_step = BumpStep::Coarse; // 0.05
        // Sit just below the boundary so a bump-up overshoots.
        p.presets[0].offsets[0].x = BipolarFloat::new(0.99);

        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);
        let msg = make_msg("/Positioner/XBumpUp", OscType::Float(1.0));
        p.control_osc_channel_scoped(&msg, &emitter).unwrap();

        // 0.99 + 0.05 = 1.04, clamped to 1.0 by BipolarFloat::new.
        assert_eq!(p.presets[0].offsets[0].x.val(), 1.0);
    }

    #[test]
    fn bump_down_clamps_at_negative_one() {
        let mut p = Positioner::default_for(1);
        p.bump_step = BumpStep::Coarse; // 0.05
        p.presets[0].offsets[0].y = BipolarFloat::new(-0.99);

        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);
        let msg = make_msg("/Positioner/YBumpDown", OscType::Float(1.0));
        p.control_osc_channel_scoped(&msg, &emitter).unwrap();

        // -0.99 + -0.05 = -1.04, clamped to -1.0.
        assert_eq!(p.presets[0].offsets[0].y.val(), -1.0);
    }

    #[test]
    fn bump_release_does_not_apply_delta() {
        // Bump buttons are momentary; the release (`0.0`) should be a no-op.
        let mut p = Positioner::default_for(1);
        p.bump_step = BumpStep::Coarse;
        p.presets[0].offsets[0].x = BipolarFloat::new(0.5);

        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);
        let msg = make_msg("/Positioner/XBumpUp", OscType::Float(0.0));
        p.control_osc_channel_scoped(&msg, &emitter).unwrap();
        assert_eq!(p.presets[0].offsets[0].x.val(), 0.5);
    }

    #[test]
    fn non_positionable_emit_clears_every_channel_scoped_control() {
        let emitter = crate::osc::MockEmitter::new();
        emit_non_positionable_channel_state(&emitter);
        let msgs = emitter.take();

        // Indexed lookups by control name for clarity.
        let by_addr: std::collections::HashMap<String, OscType> = msgs.into_iter().collect();

        assert_eq!(
            by_addr.get(addr::FIXTURE_LABEL),
            Some(&OscType::String("—".to_string())),
        );
        assert_eq!(by_addr.get(addr::X_FADER), Some(&OscType::Float(0.0)));
        assert_eq!(by_addr.get(addr::Y_FADER), Some(&OscType::Float(0.0)));
        assert_eq!(by_addr.get(addr::FOCUS_FADER), Some(&OscType::Float(0.0)));

        // Every preset radio button should be 0.0 (none selected).
        for i in 1..=N_POSITIONER_SLOTS {
            let addr = format!("{}/1/{}", addr::PRESET_SELECT.control, i);
            assert_eq!(
                by_addr.get(&addr),
                Some(&OscType::Float(0.0)),
                "preset radio {addr} not cleared",
            );
        }
        // Every preset label slot should be cleared to the empty label "".
        for i in 0..N_POSITIONER_SLOTS {
            let addr = format!("{}/{}", addr::PRESET_LABELS.control, i);
            assert_eq!(
                by_addr.get(&addr),
                Some(&OscType::String(String::new())),
                "preset label {addr} not cleared",
            );
        }
        // Every bump-step radio button should be 0.0.
        for i in 1..=3 {
            let addr = format!("{}/1/{}", addr::BUMP_STEP_SELECT.control, i);
            assert_eq!(
                by_addr.get(&addr),
                Some(&OscType::Float(0.0)),
                "bump-step radio {addr} not cleared",
            );
        }
    }
}
