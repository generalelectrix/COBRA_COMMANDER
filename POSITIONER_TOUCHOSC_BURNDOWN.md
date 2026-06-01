# Positioner — TouchOSC controls burn-down

The Rust side of the positioner is in (see commits `cf527dd` → `fb59334`
on the `positioner` branch). This document enumerates the TouchOSC
controls that still need to be hand-added to the templates before the
feature is usable on the rig.

There are two surfaces to edit:

1. `touchosc/base.touchosc` — gets a new **"Positioner"** tabpage.
2. Whichever per-fixture-type template is registered for `IWashLed` —
   gets a small **"Position Preset"** region (radio + label array).

The Rust dispatch will fall back to a cleared `"—"` state on the
channel-scoped tab whenever the current channel is non-positionable or
no channel is selected, so partial edits won't leave the iPad in a
confusing state.

---

## Address conventions

All radio buttons use `x_primary_coordinate: false` — the primary
index lives in the **second** OSC coordinate, secondary (which must be
1) lives in the first. So an N-button radio at base address `/Foo/Bar`
fires messages at `/Foo/Bar/1/1` through `/Foo/Bar/1/N`. Lay the radios
out as a vertical 1×N column in the TouchOSC editor for the cleanest
mapping.

Bipolar faders accept the full ±1.0 range. Bump buttons are momentary
(the server ignores the `0.0` release). Read-only labels are
server-driven — the show emits, the iPad displays; no input handling
needed on the template side.

---

## (1) Base template — new "Positioner" tabpage

Add to `touchosc/base.touchosc`. Address prefix: `/Positioner/`.

| OSC address | Control type | Notes |
|---|---|---|
| `/Positioner/X` | bipolar fader (±1.0) | X-axis offset for the currently-selected fixture in the active preset. |
| `/Positioner/Y` | bipolar fader (±1.0) | Y-axis offset. |
| `/Positioner/Focus` | bipolar fader (±1.0) | Focus offset. **iWashLed has no focus axis** — fader will move but DMX won't change. |
| `/Positioner/XBumpUp` | momentary button | Bump X by `+bump_step.magnitude()`. |
| `/Positioner/XBumpDown` | momentary button | Bump X by `−bump_step.magnitude()`. |
| `/Positioner/YBumpUp` | momentary button | Same for Y. |
| `/Positioner/YBumpDown` | momentary button | |
| `/Positioner/FocusBumpUp` | momentary button | Same for Focus (no DMX effect on iWashLed). |
| `/Positioner/FocusBumpDown` | momentary button | |
| `/Positioner/BumpStep/1/1`<br>`/Positioner/BumpStep/1/2`<br>`/Positioner/BumpStep/1/3` | 3-button radio (1 col × 3 rows) | Slot 1 = Coarse (~0.05), 2 = Medium (~0.01), 3 = Fine (~0.002). Default Medium. |
| `/Positioner/Prev` | momentary button | Step `selected_fixture` backward (wraps via `rem_euclid`). |
| `/Positioner/Next` | momentary button | Step `selected_fixture` forward. |
| `/Positioner/FixtureLabel` | text label (read-only) | Shows `"{selected+1} / {fixture_count}"`, or `"—"` when non-positionable / no current channel. |
| `/Positioner/Preset/1/1`<br>… `/Positioner/Preset/1/8` | 8-button radio (1 col × 8 rows) | Active preset slot. |
| `/Positioner/PresetLabel/0`<br>… `/Positioner/PresetLabel/7` | 8 text labels (read-only) | Preset names, drawn on top of the `Preset` radio buttons so every slot shows its name. Blank when non-positionable / no current channel. |
| `/Positioner/Reset` | momentary button | Zero only the *selected fixture's* offset (all 3 axes) in the active preset. |
| `/Positioner/ResetPreset` | momentary button | Zero *all fixtures'* offsets in the active preset. |

**Total: 3 faders, 6 bump buttons, 3-button radio, 2 stepper buttons,
1 label, 8-button radio, 8 preset-name labels, 2 reset buttons = 33
controls.**

---

## (2) iWashLed per-fixture-type template — "Position Preset" region

Add to whichever per-fixture-type `.touchosc` template is registered for
`IWashLed`. Author the addresses **without** the group-name prefix; the
assembly pipeline rewrites them to `/{group_name}/...` when generating
per-instance pages.

| OSC address (in template) | Resolved address (after `set_group_name`) | Control type | Notes |
|---|---|---|---|
| `PositionPresetSelect/1/1`<br>… `PositionPresetSelect/1/8` | `/{group_name}/PositionPresetSelect/1/{1..8}` | 8-button radio (1 col × 8 rows) | Per-group preset selector. Drives the same `Positioner.active` field as the channel-scoped `/Positioner/Preset` radio. |
| `PositionPresetLabel/0`<br>… `PositionPresetLabel/7` | `/{group_name}/PositionPresetLabel/{0..7}` | 8 text labels (read-only) | Preset names (`"Position 1"` through `"Position 8"` by default). Indexed 0–7 to match `LabelArray`'s zero-indexed convention. |

**Total: 8-button radio + 8 labels = 16 controls.**

Address naming is intentionally flat (`PositionPresetSelect`, not
`PositionPreset/Select`) so the standard `RadioButton` and `LabelArray`
primitives parse them directly — same convention as the existing
`Animation/TargetLabel` pattern.

---

## Quick sanity checklist when you wire things up

- [ ] Press `/Positioner/X` fader on the channel-scoped tab while
      iWashLed is the current channel → DMX pan moves for the selected
      fixture.
- [ ] Press `/Positioner/Y` → tilt moves.
- [ ] Press `/Positioner/Focus` → nothing visible in DMX (iWashLed has
      no focus axis); the value is silently stored but never contributes
      to render.
- [ ] Tap `/Positioner/Preset/1/3` → DMX snaps to whatever offsets are
      in slot 2 (1-indexed button → 0-indexed slot); per-group radio on
      the iWashLed page lights button 3.
- [ ] Tap `/IWashFront/PositionPresetSelect/1/5` while iWashFront IS the
      current channel → both the per-group radio AND the channel-scoped
      `/Positioner/Preset` radio light button 5; DMX snaps.
- [ ] Tap `/IWashFront/PositionPresetSelect/1/5` while iWashBack IS the
      current channel → only iWashFront's per-group radio lights;
      channel-scoped tab is unchanged.
- [ ] Switch channels via `/Channels/Select` → `/Positioner/...` state
      refreshes to reflect the new channel's positioner, including the 8
      `PresetLabel/{0..7}` slots; FixtureLabel reads `"—"` and the
      preset labels blank out if the new channel is non-positionable.
- [ ] On the desktop **Positioner** tab, type a name + Enter → the active
      preset's name updates on both the channel-scoped
      `/Positioner/PresetLabel/{active}` slot and the per-group
      `/{group_name}/PositionPresetLabel/{active}` slot.
- [ ] Set offsets in a preset, then repatch the iWashLed group with a
      different fixture count → offsets preserved where they overlap;
      new fixtures land at zero; dropped fixtures' offsets vanish.
