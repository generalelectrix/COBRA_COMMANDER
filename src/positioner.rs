//! Per-fixture position offsets for moving heads.
//!
//! Each positionable [`crate::fixture::FixtureGroup`] owns a [`Positioner`]
//! that stores per-fixture `(x, y, focus)` offsets across 8 named preset
//! slots. The render pipeline reads these offsets and contributes them as
//! additional animation values via the
//! [`crate::fixture::animation_target::TargetedAnimationValues::chain`]
//! combinator, so the existing animation summing in `val_with_anim` handles
//! "ride along with animations" semantics for free.
//!
//! See the design plan for the full picture. This module currently exposes
//! only the data model and construction/reconciliation helpers; OSC dispatch
//! and emit logic land in a follow-up.

use number::BipolarFloat;

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
    #[cfg_attr(not(test), expect(dead_code))] // Read by OSC dispatch in step 3.
    pub bump_step: BumpStep,
}

/// One preset slot's data: a name and a per-fixture offset vector.
#[derive(Debug, Clone)]
pub struct PositionPreset {
    /// Always populated. Defaults to `"Position {1..8}"` until the operator
    /// renames it via the desktop GUI.
    #[cfg_attr(not(test), expect(dead_code))]
    // Read by OSC emit and rename handler in steps 3 and 4.
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
#[cfg_attr(not(test), expect(dead_code))] // Coarse/Fine used by OSC dispatch (step 3); Medium used at construction.
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
    #[cfg_attr(not(test), expect(dead_code))] // Used by OSC dispatch in step 3.
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
#[expect(dead_code)] // Used by OSC dispatch in step 3.
pub enum Axis {
    X,
    Y,
    Focus,
}

/// Sign of a bump delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(dead_code)] // Used by OSC dispatch in step 3.
pub enum Sign {
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
        }
    }

    /// Grow or shrink each preset's `offsets` vector to match a new fixture
    /// count, preserving existing values where they overlap. Used during
    /// repatch reconciliation when a positionable group gains or loses
    /// fixtures.
    ///
    /// Extending pads with default (zero) offsets; truncating drops the tail
    /// entries.
    #[cfg_attr(not(test), expect(dead_code))] // Wired into reconfigure_from in step 4.
    pub fn reconcile_to_fixture_count(&mut self, new_count: usize) {
        for preset in &mut self.presets {
            preset
                .offsets
                .resize_with(new_count, PositionOffset::default);
        }
        // If a shrink dropped the previously-selected fixture, clamp.
        if self.selected_fixture >= new_count {
            self.selected_fixture = new_count.saturating_sub(1);
        }
    }
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
}
