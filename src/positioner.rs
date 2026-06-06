//! Per-fixture position offsets for moving heads.
//!
//! Each positionable [`crate::fixture::FixtureGroup`] owns a [`Positioner`]
//! that stores per-fixture `(x, y, focus)` offsets across 8 named preset
//! slots, plus the editing state (selected fixture, bump step) shown on
//! the Positioner tab.

use anyhow::{Result, bail};
use number::BipolarFloat;
use rosc::OscType;
use serde::{Deserialize, Serialize};

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
/// the editing state (selected fixture, bump granularity) shown on the
/// Positioner tab.
#[derive(Debug, Clone)]
pub struct Positioner {
    presets: PositionerPresets,
    /// Active preset slot (`0..N_POSITIONER_SLOTS`).
    active: usize,
    /// Index of the fixture being edited via the Positioner tab
    /// (`0..fixture_count`).
    selected_fixture: usize,
    /// Step magnitude for the Positioner tab's bump buttons.
    bump_step: BumpStep,
    /// Number of fixtures this positioner is sized for; always equals every
    /// preset's `offsets.len()`.
    fixture_count: usize,
}

/// The persistable subset of a `Positioner`: the named preset slots.
/// Editing UI state (active slot, selected fixture, bump step) is
/// session-only and is not part of this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionerPresets {
    pub slots: [PositionPreset; N_POSITIONER_SLOTS],
}

/// One preset slot's data: a name and a per-fixture offset vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
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

/// Step magnitude for the Positioner tab's bump buttons. Same step applies
/// to X, Y, and Focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::ops::Index<usize> for PositionerPresets {
    type Output = PositionPreset;
    fn index(&self, i: usize) -> &PositionPreset {
        &self.slots[i]
    }
}

impl std::ops::IndexMut<usize> for PositionerPresets {
    fn index_mut(&mut self, i: usize) -> &mut PositionPreset {
        &mut self.slots[i]
    }
}

impl PositionerPresets {
    /// Build 8 default preset slots (named `"Position 1"` through
    /// `"Position 8"`) sized for `fixture_count` fixtures.
    pub fn default_for(fixture_count: usize) -> Self {
        let slots = std::array::from_fn(|i| PositionPreset {
            name: format!("Position {}", i + 1),
            offsets: vec![PositionOffset::default(); fixture_count],
        });
        Self { slots }
    }

    /// Grow or shrink each preset's `offsets` vector to match a new
    /// fixture count, preserving existing values where they overlap.
    /// Extending pads with default (zero) offsets; truncating drops the
    /// tail entries.
    pub fn reconcile_to_fixture_count(&mut self, fixture_count: usize) {
        for slot in &mut self.slots {
            slot.offsets
                .resize_with(fixture_count, PositionOffset::default);
        }
    }
}

impl Positioner {
    /// Build a fresh positioner for a group with `fixture_count` fixtures.
    ///
    /// All 8 preset slots are initialized with the default name (`"Position
    /// 1"` through `"Position 8"`) and `fixture_count` zeroed offsets each.
    /// `active = 0`, `selected_fixture = 0`, `bump_step = Medium`.
    pub fn default_for(fixture_count: usize) -> Self {
        Self {
            presets: PositionerPresets::default_for(fixture_count),
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
        self.presets.reconcile_to_fixture_count(new_count);
        self.fixture_count = new_count;
        // If a shrink dropped the previously-selected fixture, clamp.
        if self.selected_fixture >= new_count {
            self.selected_fixture = new_count.saturating_sub(1);
        }
    }

    /// The persisted subset of this positioner's state (the named preset
    /// slots).
    pub fn presets(&self) -> &PositionerPresets {
        &self.presets
    }

    /// Install loaded preset data, reconciling it to the current fixture
    /// count first.
    pub fn install_presets(&mut self, mut presets: PositionerPresets) {
        presets.reconcile_to_fixture_count(self.fixture_count);
        self.presets = presets;
    }

    /// The offset for a given fixture in the currently-active preset, or
    /// `None` if `fixture_index` is out of range.
    pub fn offset_for_fixture(&self, fixture_index: usize) -> Option<PositionOffset> {
        self.presets
            .slots
            .get(self.active)
            .and_then(|preset| preset.offsets.get(fixture_index))
            .copied()
    }

    /// Rename the currently-active preset slot and push the one label slot
    /// that changed on the per-group preset selector and (when the addressed
    /// group is the current channel) the Positioner tab. No-op if `active`
    /// is out of range.
    pub fn rename_active_preset(&mut self, name: String, emitter: &FixtureStateEmitter) {
        let active = self.active;
        let Some(preset) = self.presets.slots.get_mut(active) else {
            return;
        };
        preset.name = name.clone();
        addr::POSITION_PRESET_LABEL.set_one(active, name.clone(), emitter);
        if emitter.channel().is_current() {
            addr::PRESET_LABELS.set_one(active, name, &emitter.scoped(addr::GROUP));
        }
    }
}

/// Build an `anyhow::Error` for a violated positioner invariant. The `code`
/// (of the form `"PO-NNN"`) makes the originating site grep-able.
fn positioner_inconsistency(code: &'static str, details: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(
        "Error code: {code}. Positioner inconsistency: {details}. \
         This is a bug — please report to this application's developers."
    )
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
        if msg.control() == addr::POSITION_PRESET_SELECT.control {
            Some(self.handle_preset_select(msg, &addr::POSITION_PRESET_SELECT, emitter))
        } else {
            None
        }
    }

    /// Handle a Positioner-tab OSC message (X/Y/Focus faders and bumps,
    /// BumpStep, Prev/Next, Preset, Reset, ResetPreset). Returns `Err` for
    /// an unrecognized address or a recognized-but-malformed message.
    pub fn control_osc_positioner_scoped(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        match msg.control() {
            addr::X_FADER => self.handle_fader(msg, Axis::X, emitter),
            addr::Y_FADER => self.handle_fader(msg, Axis::Y, emitter),
            addr::FOCUS_FADER => self.handle_fader(msg, Axis::Focus, emitter),

            addr::X_BUMP_UP => self.handle_bump(msg, Axis::X, Sign::Plus, emitter),
            addr::X_BUMP_DOWN => self.handle_bump(msg, Axis::X, Sign::Minus, emitter),
            addr::Y_BUMP_UP => self.handle_bump(msg, Axis::Y, Sign::Plus, emitter),
            addr::Y_BUMP_DOWN => self.handle_bump(msg, Axis::Y, Sign::Minus, emitter),
            addr::FOCUS_BUMP_UP => self.handle_bump(msg, Axis::Focus, Sign::Plus, emitter),
            addr::FOCUS_BUMP_DOWN => self.handle_bump(msg, Axis::Focus, Sign::Minus, emitter),

            c if c == addr::BUMP_STEP_SELECT.control => self.handle_bump_step_select(msg, emitter),

            addr::PREV_FIXTURE => self.handle_nudge_fixture(msg, Sign::Minus, emitter),
            addr::NEXT_FIXTURE => self.handle_nudge_fixture(msg, Sign::Plus, emitter),

            c if c == addr::PRESET_SELECT.control => {
                self.handle_preset_select(msg, &addr::PRESET_SELECT, emitter)
            }

            addr::RESET_FIXTURE => self.handle_reset_fixture(msg, emitter),
            addr::RESET_PRESET => self.handle_reset_preset(msg, emitter),

            other => bail!("unrecognized Positioner-tab control: {other}"),
        }
    }

    /// The offset for the selected fixture in the active preset. Returns
    /// `Ok(None)` when there are no patched fixtures (legitimate no-op).
    /// Returns `Err` for an out-of-range `active` or `selected_fixture`,
    /// both of which are invariant violations.
    fn selected_offset_mut(&mut self) -> Result<Option<&mut PositionOffset>> {
        if self.fixture_count == 0 {
            return Ok(None);
        }
        let active = self.active;
        let selected_fixture = self.selected_fixture;
        let preset = self.presets.slots.get_mut(active).ok_or_else(|| {
            positioner_inconsistency(
                "PO-001",
                format!("active preset slot {active} out of range (max {N_POSITIONER_SLOTS})"),
            )
        })?;
        let fixture_count = preset.offsets.len();
        preset
            .offsets
            .get_mut(selected_fixture)
            .map(Some)
            .ok_or_else(|| {
                positioner_inconsistency(
                    "PO-002",
                    format!(
                        "selected fixture index {selected_fixture} out of range \
                     (fixture_count {fixture_count})",
                    ),
                )
            })
    }

    fn handle_fader(
        &mut self,
        msg: &OscControlMessage,
        axis: Axis,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        let val = msg.get_bipolar()?;
        let Some(offset) = self.selected_offset_mut()? else {
            return Ok(());
        };
        match axis {
            Axis::X => offset.x = val,
            Axis::Y => offset.y = val,
            Axis::Focus => offset.focus = val,
        }
        self.emit_axis(axis, &emitter.scoped(addr::GROUP));
        Ok(())
    }

    fn handle_bump(
        &mut self,
        msg: &OscControlMessage,
        axis: Axis,
        sign: Sign,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        if !msg.get_bool()? {
            return Ok(());
        }
        let signed_delta = match sign {
            Sign::Plus => self.bump_step.magnitude(),
            Sign::Minus => -self.bump_step.magnitude(),
        };
        let Some(offset) = self.selected_offset_mut()? else {
            return Ok(());
        };
        match axis {
            Axis::X => offset.x = BipolarFloat::new(offset.x.val() + signed_delta),
            Axis::Y => offset.y = BipolarFloat::new(offset.y.val() + signed_delta),
            Axis::Focus => offset.focus = BipolarFloat::new(offset.focus.val() + signed_delta),
        }
        self.emit_axis(axis, &emitter.scoped(addr::GROUP));
        Ok(())
    }

    fn handle_bump_step_select(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        let Some(index) = addr::BUMP_STEP_SELECT.parse_press(msg)? else {
            return Ok(());
        };
        self.bump_step = match index {
            0 => BumpStep::Coarse,
            1 => BumpStep::Medium,
            2 => BumpStep::Fine,
            _ => return Ok(()),
        };
        self.emit_bump_step_radio(&emitter.scoped(addr::GROUP));
        Ok(())
    }

    fn handle_nudge_fixture(
        &mut self,
        msg: &OscControlMessage,
        sign: Sign,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        if !msg.get_bool()? || self.fixture_count == 0 {
            return Ok(());
        }
        let delta: isize = match sign {
            Sign::Plus => 1,
            Sign::Minus => -1,
        };
        let new = (self.selected_fixture as isize + delta).rem_euclid(self.fixture_count as isize);
        self.selected_fixture = new as usize;
        let scoped = emitter.scoped(addr::GROUP);
        self.emit_fixture_label(&scoped);
        self.emit_selected_axes(&scoped);
        Ok(())
    }

    fn handle_preset_select(
        &mut self,
        msg: &OscControlMessage,
        primitive: &RadioButton,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        let Some(index) = primitive.parse_press(msg)? else {
            return Ok(());
        };
        if index >= N_POSITIONER_SLOTS || self.active == index {
            return Ok(());
        }
        self.active = index;
        // The per-group preset radio always reflects the change (it's the
        // surface this came from when dispatched per-group, and it tracks
        // active state regardless when dispatched positioner-scoped).
        self.emit_per_group_state(emitter);
        // The Positioner tab only reflects state for the current channel.
        if emitter.channel().is_current() {
            self.emit_positioner_state(&emitter.scoped(addr::GROUP));
        }
        Ok(())
    }

    fn handle_reset_fixture(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        if !msg.get_bool()? {
            return Ok(());
        }
        let Some(offset) = self.selected_offset_mut()? else {
            return Ok(());
        };
        *offset = PositionOffset::default();
        self.emit_selected_axes(&emitter.scoped(addr::GROUP));
        Ok(())
    }

    fn handle_reset_preset(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> Result<()> {
        if !msg.get_bool()? {
            return Ok(());
        }
        let active = self.active;
        let preset = self.presets.slots.get_mut(active).ok_or_else(|| {
            positioner_inconsistency(
                "PO-001",
                format!("active preset slot {active} out of range (max {N_POSITIONER_SLOTS})"),
            )
        })?;
        for off in &mut preset.offsets {
            *off = PositionOffset::default();
        }
        self.emit_selected_axes(&emitter.scoped(addr::GROUP));
        Ok(())
    }

    /// Push the Positioner tab state. The emitter should be scoped to the
    /// [`addr::GROUP`] entity.
    pub fn emit_positioner_state<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        self.emit_fixture_label(emitter);
        self.emit_selected_axes(emitter);
        addr::PRESET_SELECT.set(self.active, false, emitter);
        addr::PRESET_LABELS.set(self.presets.slots.iter().map(|p| p.name.clone()), emitter);
        self.emit_bump_step_radio(emitter);
    }

    /// Push the per-group preset selector state (radio index + 8 labels).
    /// The emitter should be scoped to the group's name (e.g. via the
    /// [`FixtureStateEmitter`] that prefixes addresses with the group name).
    pub fn emit_per_group_state<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        addr::POSITION_PRESET_SELECT.set(self.active, false, emitter);
        addr::POSITION_PRESET_LABEL.set(self.presets.slots.iter().map(|p| p.name.clone()), emitter);
    }

    /// Push the selected fixture's offset value along one axis.
    fn emit_axis<E: EmitScopedOscMessage + ?Sized>(&self, axis: Axis, emitter: &E) {
        let val = self
            .presets
            .slots
            .get(self.active)
            .and_then(|preset| preset.offsets.get(self.selected_fixture))
            .map(|off| match axis {
                Axis::X => off.x.val(),
                Axis::Y => off.y.val(),
                Axis::Focus => off.focus.val(),
            })
            .unwrap_or(0.0);
        emitter.emit_float(axis.fader_addr(), val);
    }

    /// Push all three offset faders for the selected fixture.
    fn emit_selected_axes<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        self.emit_axis(Axis::X, emitter);
        self.emit_axis(Axis::Y, emitter);
        self.emit_axis(Axis::Focus, emitter);
    }

    /// Push the `"{selected_fixture + 1} / {fixture_count}"` label, or
    /// `"—"` when the group has no patched fixtures.
    fn emit_fixture_label<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        let label = if self.fixture_count == 0 {
            "—".to_string()
        } else {
            format!("{} / {}", self.selected_fixture + 1, self.fixture_count)
        };
        emitter.emit_osc(ScopedOscMessage {
            control: addr::FIXTURE_LABEL,
            arg: OscType::String(label),
        });
    }

    /// Push the bump-step radio selection (Coarse / Medium / Fine).
    fn emit_bump_step_radio<E: EmitScopedOscMessage + ?Sized>(&self, emitter: &E) {
        let bump_index = match self.bump_step {
            BumpStep::Coarse => 0,
            BumpStep::Medium => 1,
            BumpStep::Fine => 2,
        };
        addr::BUMP_STEP_SELECT.set(bump_index, false, emitter);
    }
}

impl Axis {
    fn fader_addr(self) -> &'static str {
        match self {
            Self::X => addr::X_FADER,
            Self::Y => addr::Y_FADER,
            Self::Focus => addr::FOCUS_FADER,
        }
    }
}

/// Push neutral / cleared values for every Positioner-tab control:
/// FixtureLabel reads `"—"`, faders snap to 0, both radios fully
/// deselect. The emitter should be scoped to [`addr::GROUP`].
pub fn emit_cleared_positioner_state<E: EmitScopedOscMessage + ?Sized>(emitter: &E) {
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

    /// Test-only accessors for fields that production code has no business
    /// reading or writing. These are pub so test modules outside this file
    /// (e.g. `show.rs` integration tests, `patch::repatch` regression tests)
    /// can still reach into a `Positioner`.
    impl Positioner {
        pub fn active(&self) -> usize {
            self.active
        }

        pub fn selected_fixture(&self) -> usize {
            self.selected_fixture
        }

        pub fn presets_mut(&mut self) -> &mut PositionerPresets {
            &mut self.presets
        }

        pub fn set_active(&mut self, active: usize) {
            self.active = active;
        }

        pub fn set_selected_fixture(&mut self, idx: usize) {
            self.selected_fixture = idx;
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

        // Shrinking to zero fixtures is its own edge case (empty offsets vec).
        p.reconcile_to_fixture_count(0);
        assert_eq!(p.presets[0].offsets.len(), 0);
        assert_eq!(p.selected_fixture, 0);
    }

    #[test]
    fn positioner_yaml_round_trip() {
        let mut p = Positioner::default_for(3);
        p.presets_mut()[0].offsets[0].x = BipolarFloat::new(0.5);
        p.presets_mut()[0].offsets[1].y = BipolarFloat::new(-0.25);
        p.presets_mut()[2].offsets[2].focus = BipolarFloat::new(0.1);
        p.presets_mut()[3].name = "Bar Spots".to_string();
        p.set_active(2);
        p.set_selected_fixture(1);
        p.bump_step = BumpStep::Coarse;

        // Only the persistable subset survives a save/load cycle. Editing
        // UI state (active, selected_fixture, bump_step) is session-only.
        let yaml = serde_yaml::to_string(p.presets()).unwrap();
        let restored: PositionerPresets = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(restored[0].offsets[0].x.val(), 0.5);
        assert_eq!(restored[0].offsets[1].y.val(), -0.25);
        assert_eq!(restored[2].offsets[2].focus.val(), 0.1);
        assert_eq!(restored[3].name, "Bar Spots");
        for preset in &restored.slots {
            assert_eq!(preset.offsets.len(), 3);
        }
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

    /// Regression: per-group dispatch must NOT match the Positioner tab's
    /// vocabulary. Otherwise a fixture's own `/MyFixture/Focus` (or
    /// `/MyFixture/Reset`, `/MyFixture/X`, etc.) would be swallowed by
    /// the positioner before reaching the fixture's own control handler.
    #[test]
    fn control_osc_per_group_ignores_positioner_scoped_and_unknown_addresses() {
        let mut p = Positioner::default_for(4);
        let name = crate::config::GroupName("MyFixture".to_string());
        let emitter = null_fixture_emitter(&name);

        // Positioner-tab controls that a real fixture might collide with.
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
    fn control_osc_positioner_scoped_errors_on_unknown_address() {
        let mut p = Positioner::default_for(1);
        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);

        let msg = make_msg("/Positioner/NotAControl", OscType::Float(1.0));
        let err = p
            .control_osc_positioner_scoped(&msg, &emitter)
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
        p.control_osc_positioner_scoped(&msg, &emitter).unwrap();

        // 0.99 + 0.05 = 1.04, clamped to 1.0 by BipolarFloat::new.
        assert_eq!(p.presets[0].offsets[0].x.val(), 1.0);
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
        p.control_osc_positioner_scoped(&msg, &emitter).unwrap();
        assert_eq!(p.presets[0].offsets[0].x.val(), 0.5);
    }

    /// A press handler with `selected_fixture` out of range relative to
    /// `fixture_count` should surface a `PO-002` inconsistency rather than
    /// silently swallow the operator's input.
    #[test]
    fn fader_with_corrupted_selected_fixture_errors() {
        let mut p = Positioner::default_for(2);
        p.selected_fixture = 99; // Violate the invariant.

        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);
        let msg = make_msg("/Positioner/X", OscType::Float(0.5));
        let err = p
            .control_osc_positioner_scoped(&msg, &emitter)
            .expect_err("should error on out-of-range selected_fixture");
        let msg = err.to_string();
        assert!(msg.contains("PO-002"), "wanted PO-002 in: {msg}");
        assert!(msg.contains("99"), "wanted bad index in: {msg}");
    }

    #[test]
    fn fader_on_empty_group_is_silent_noop() {
        // fixture_count == 0 is legitimate (group patched with no fixtures);
        // a fader write should silently do nothing and NOT error.
        let mut p = Positioner::default_for(0);

        let name = crate::config::GroupName("Test".to_string());
        let emitter = null_fixture_emitter(&name);
        let msg = make_msg("/Positioner/X", OscType::Float(0.5));
        p.control_osc_positioner_scoped(&msg, &emitter).unwrap();
    }
}
