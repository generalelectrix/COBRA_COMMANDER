//! OSC address constants and primitive declarations for the positioner.
//!
//! The positioner has two OSC address spaces, both handled by the same
//! `Positioner::control_osc` method (which the OSC layer dispatches to):
//!
//! - **Channel-scoped** (`/Positioner/...`): the operator's editing surface
//!   for the currently-selected channel. Faders, bumps, fixture stepper,
//!   preset radio, etc.
//! - **Per-group** (`/{group_name}/PositionPreset/...`): a small region
//!   embedded inside each positionable fixture type's own TouchOSC template,
//!   so iPads showing a specific group's controls page can switch its
//!   active preset independently.
//!
//! Both vocabularies are non-overlapping after prefix stripping — see
//! `Positioner::control_osc` for the unified dispatch.

use super::label_array::LabelArray;
use super::radio_button::RadioButton;

/// Channel-scoped OSC group identifier — the entity name for the
/// Positioner tab in the base TouchOSC template.
pub const GROUP: &str = "Positioner";

// === Channel-scoped controls (under `/Positioner/...`) ===

/// Per-axis bipolar offset faders. The address is `/Positioner/{name}`
/// where `{name}` is one of these.
pub const X_FADER: &str = "X";
pub const Y_FADER: &str = "Y";
pub const FOCUS_FADER: &str = "Focus";

/// Per-axis bump buttons. Address pattern `/Positioner/{Axis}Bump{Up,Down}`.
pub const X_BUMP_UP: &str = "XBumpUp";
pub const X_BUMP_DOWN: &str = "XBumpDown";
pub const Y_BUMP_UP: &str = "YBumpUp";
pub const Y_BUMP_DOWN: &str = "YBumpDown";
pub const FOCUS_BUMP_UP: &str = "FocusBumpUp";
pub const FOCUS_BUMP_DOWN: &str = "FocusBumpDown";

/// 3-button radio selecting the bump step magnitude (Coarse / Medium / Fine).
/// Address pattern `/Positioner/BumpStep/{1..3}/1`.
pub const BUMP_STEP_SELECT: RadioButton = RadioButton {
    control: "BumpStep",
    n: 3,
    x_primary_coordinate: false,
};

/// Step the selected fixture index backward / forward.
pub const PREV_FIXTURE: &str = "Prev";
pub const NEXT_FIXTURE: &str = "Next";

/// Read-only label showing `"{selected_fixture + 1} / {fixture_count}"`.
pub const FIXTURE_LABEL: &str = "FixtureLabel";

/// 8-button radio selecting the active preset slot for the current
/// channel's group. Address pattern `/Positioner/Preset/{1..8}/1`.
pub const PRESET_SELECT: RadioButton = RadioButton {
    control: "Preset",
    n: crate::positioner::N_POSITIONER_SLOTS,
    x_primary_coordinate: false,
};

/// 8-slot label array of preset names on the channel-scoped Positioner tab.
/// Address pattern `/Positioner/PresetLabel/{0..7}`. Drawn on top of the
/// `PRESET_SELECT` radio in the TouchOSC layout, so the operator can see
/// every slot's name (not just the active one).
pub const PRESET_LABELS: LabelArray = LabelArray {
    control: "PresetLabel",
    n: crate::positioner::N_POSITIONER_SLOTS,
    empty_label: "",
};

/// Zero the selected fixture's offset (all three axes) in the active preset.
pub const RESET_FIXTURE: &str = "Reset";
/// Zero all offsets in the active preset.
pub const RESET_PRESET: &str = "ResetPreset";

// === Per-group controls (under `/{group_name}/...`) ===
//
// Address naming is flat (`PositionPresetSelect`, `PositionPresetLabel`)
// rather than nested (`PositionPreset/Select`, `PositionPreset/Label`) so the
// standard `RadioButton` and `LabelArray` primitives parse them directly.
// TouchOSC visual grouping of "Position Preset" controls is independent of
// the address naming. Matches the existing `Animation/TargetLabel` pattern
// rather than a nested `Animation/Target/Label`.

/// Per-group 8-button preset radio. Address pattern
/// `/{group_name}/PositionPresetSelect/{1..8}/1`.
pub const POSITION_PRESET_SELECT: RadioButton = RadioButton {
    control: "PositionPresetSelect",
    n: crate::positioner::N_POSITIONER_SLOTS,
    x_primary_coordinate: false,
};

/// Per-group 8-slot label array of preset names. Address pattern
/// `/{group_name}/PositionPresetLabel/{0..7}`.
pub const POSITION_PRESET_LABEL: LabelArray = LabelArray {
    control: "PositionPresetLabel",
    n: crate::positioner::N_POSITIONER_SLOTS,
    empty_label: "",
};
