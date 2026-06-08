# Control reference — what to expose, and how

## Feature inclusion idiom (what to expose vs pin/drop)

Cobra is improvisation-first. Every exposed control must be something a performer varies in
real time. Decide per DMX channel:

| Fixture feature | Cobra treatment |
|---|---|
| **Built-in strobe / shutter** | **Ignore.** Pin the channel to its "open / no strobe" value. Strobing is global, via `.strobed()` on the dimmer + `#[strobe(Short\|Long)]` on the struct. |
| **Dimmer / intensity** | **Always a live control.** `Unipolar::full_channel("Dimmer", ch).strobed().with_channel_level()`. This is the one channel-level control. If brightness is RGB-only (no dimmer channel), pin the dimmer to full and drive brightness via color. |
| **Macro / auto-program / sound-active / reset / "AUTO" / "REST"** | **Pin to 0** (or the documented "no-op" value). Antithetical to live control. |
| **Pan/tilt speed ("XY speed")** | **Pin to fastest** (usually 0). Speed comes from animations, not firmware. |
| **Color wheel** | Discrete slots only → `LabeledSelect`. Drop the continuous rainbow/scroll/"flow" range. If the wheel has split positions, use `.with_split(offset)`. |
| **Gobo wheel(s)** | Discrete slots only → `IndexedSelect::multiple`. Drop shake and scroll/flow ranges. A fixture may have two wheels (a *fixed* and a *rotating* one) — expose both. |
| **Gobo / prism / mirror rotation** | `Bipolar::split_channel(...).with_detent().with_mirroring(true).with_channel_knob(i)` — CW/stop/CCW. Drop "bounce"/effect ranges. |
| **Focus / iris** | Unipolar. If it benefits from positioner-driven sweeps, model focus as **bipolar** (`BipolarChannel`) so it rests at mid-throw and rides the positioner's bipolar focus offset (see positioner axes below). Not channel-level (the dimmer owns that), not a knob. |
| **Prism** | `Bool` insert that **gates** a rotation control sharing the same channel (the "twinkle" pattern, below). Map: prism-out value when the Bool is off; insert+rotation range when on. |
| **Mirror (pan/tilt/rotation invert)** | Set-once-by-physical-geometry. `.with_mirroring(default_on)` adds a `Mirror<Name>` toggle. May be omitted from the TouchOSC template for a given show (the OSC control still exists). Conceptually belongs in patch config, not the live surface. |

## The manual is frequently wrong — verify on hardware

Observed failure modes (all happened in real profiles):
- **Wrong channel order** (e.g. claimed pan-coarse/tilt-coarse/pan-fine/tilt-fine interleaved;
  actually adjacent coarse+fine like every other moving head).
- **Wrong personality / wrong manual entirely** — confirm the channel count matches the head.
- **Over-counted wheels** — manual listed 16 colors / 8 gobos; the physical wheel had 8 / 7.
- **"Open" position** is usually the wheel's first slot (manual's "Color1"/"Gobo1" is often open).
- **Shutter/strobe "open" value** varies — sometimes 0 (neutral), sometimes a high "LED on"
  value is required for the dimmer to output anything. Confirm light actually comes out.

Leave generic placeholder names (`color1`, …) and evenly-spaced placeholder DMX values where
the owner will map them with the real fixture; they'll hand you the real list later.

## Control-type cheatsheet

All live in `src/fixture/control/` and are re-exported via `crate::fixture::prelude::*`.

- **`LabeledSelect`** — discrete named slots (color/gobo by name).
  `LabeledSelect::new("Color", ch, vec![("Open",0),("Red",20),…])`.
  `.with_split(offset)` adds a `Split<Name>` Bool that adds `offset` to the rendered value
  (half-step split between adjacent slots).
- **`IndexedSelect` / `IndexedSelectMult`** — a grid of indexed slots (gobos by index).
  `IndexedSelect::multiple("Gobo", ch, x_primary, n, mult, offset)` → `dmx = index*mult + offset`.
  Pick `mult`/`offset` so each index lands in its band (index 0 is usually "open").
- **`Bool<()>`** — a toggle that holds state but renders nothing itself (gates other controls).
  `Bool::new_off("Prism", ())`. `Bool::channel`/`full_channel` render to a DMX channel directly.
- **`Unipolar` / `UnipolarChannel`** — 0..1 fader. `Unipolar::full_channel("X", ch)` (0–255) or
  `Unipolar::channel("X", ch, start, end)`. `.strobed()` routes global strobe through it.
  `.with_channel_level()` → `ChannelLevelUnipolar` (hardware level fader; use for the dimmer).
  `.with_channel_knob(i)` → `ChannelKnobUnipolar` (hardware knob).
- **`Bipolar` / `BipolarChannel`** — -1..1.
  `Bipolar::channel("Focus", ch, start, end)` (continuous; 0.0 → mid).
  `Bipolar::split_channel("Rot", ch, cw_slow, cw_fast, ccw_slow, ccw_fast, stop)` (CW/stop/CCW).
  `Bipolar::coarse_fine("Pan", ch)` (16-bit across ch, ch+1).
  `.with_detent()` (5% null at center), `.with_mirroring(default_on)` → `Mirrored`,
  `.with_channel_knob(i)` → `ChannelKnobBipolar`.
- **`Mirrored<R>`** — wraps a bipolar with a `Mirror<Name>` toggle; used for pan/tilt/rotation.

## Struct attributes & derives

```rust
#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = N]      // total DMX footprint; all offsets must fit in [0, N)
#[strobe(Short)]          // Short = 1-frame flash (LED), Long = 3-frame (slower fixtures)
#[no_touchosc_template]   // only while the .touchosc file doesn't exist yet
```
- `#[channel_control]` — wire this field to hardware faders/knobs (needs a ChannelLevel/Knob type).
- `#[animate]` — add the field to the generated `AnimationTarget` enum (animatable + positioner).
- `#[animate_subtarget(Hue, Sat, Val)]` — for embedded `Color`-style sub-fixtures.

`#[derive(PatchFixture)]` auto-registers the patcher (linkme `PATCHERS`) **and** looks for
`touchosc/group_templates/<StructName>.touchosc` at compile time.

## Positioner axes (moving heads)

```rust
fn positioner_axes() -> Option<PositionerAxes<Self::Target>> {
    Some(PositionerAxes {
        x: AnimationTarget::Pan,
        y: AnimationTarget::Tilt,
        focus: Some(AnimationTarget::Focus), // or None if no focus / not positioner-driven
    })
}
```
The focus offset is a `BipolarFloat` applied as an animation value — model focus as
`BipolarChannel` so it rests centered and the offset reads naturally.

## The "shared channel / gate" pattern (prism, twinkle)

When one DMX channel does two jobs (e.g. insert + rotation speed), use a `Bool` to gate a
second control that renders to the same channel — render the second control only when the Bool
is on, else write the "off" value. (Mirrors `wizard_extreme.rs`'s twinkle/twinkle_speed.)

```rust
if self.prism.val() {
    self.prism_rotation.render(group_controls, anim.filter(&AnimationTarget::PrismRotation), dmx);
} else {
    dmx_buf[ch] = 0; // prism out
}
```

## No panics (repo rule)

Production code must not panic: no `.unwrap()`/`.expect()`/indexing that can fail/`panic!`/
`todo!`. In `render_with_animations`, direct `dmx_buf[i] = …` is the established idiom (the
buffer is pre-sized to `channel_count`), but everything else must use `let Some(..) else`,
`.get()`, `?`, etc.
