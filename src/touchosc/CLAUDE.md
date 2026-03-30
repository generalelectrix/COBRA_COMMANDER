# TouchOSC File Format — Coordinate System Notes

## File Structure

A `.touchosc` file is a ZIP archive containing a single `index.xml`. The XML
has `<layout>` → `<tabpage>` → `<control>` elements. String values (names, OSC
addresses, label text) are base64-encoded. MIDI bindings are `<midi ... />`
children of controls with a quirky `var ="x"` (space before `=`) format.

## The 90-Degree Rotation Problem

TouchOSC's XML stores coordinates in **portrait orientation**, but the editor
can display the layout in **landscape**. When `orientation="vertical"` in the
XML, the editor renders it as a **landscape** (horizontal) layout by applying a
90-degree counter-clockwise rotation. This means the XML coordinate system does
not match what you see in the editor.

### XML coordinates vs editor (landscape) appearance

```
XML portrait frame:          Editor landscape frame:

  x=0          x=730           ┌─────────────────────┐
  ┌──────────┐                 │ top-left   top-right │
  │          │  y=0            │                      │
  │          │                 │                      │
  │          │                 │ bot-left   bot-right │
  │          │  y=1024         └─────────────────────┘
  └──────────┘
```

In the XML:
- `x` axis spans 0→730 (the short portrait axis)
- `y` axis spans 0→1024 (the long portrait axis)

After the editor's 90° CCW rotation to landscape:
- **XML x increases UPWARD** in the editor (x=0 is bottom, x=730 is top)
- **XML y increases RIGHTWARD** in the editor (y=0 is left, y=1024 is right)
- **XML `w` is the VERTICAL extent** (height in the editor)
- **XML `h` is the HORIZONTAL extent** (width in the editor)

### Label orientation is also rotated

- `labelv` → text reads **left-to-right** in the landscape editor (this is the one you usually want)
- `labelh` → text reads **bottom-to-top** in the landscape editor

This is backwards from what the names suggest, because `labelv`/`labelh` refer
to the portrait orientation, not what you see in landscape.

### The `orientation` attribute is inverted

The XML attribute `orientation="vertical"` causes the editor to display the
layout in **landscape** (horizontal) mode. `orientation="horizontal"` would
display as portrait. All of the Cobra Commander layouts use
`orientation="vertical"` (i.e., landscape in the editor).

## Our Model

We store XML coordinates **as-is** with no transformation. This means:

- Round-trip fidelity is trivial (parse and serialize are identity on coordinates)
- When creating new controls for landscape layouts, remember:
  - "move right" = increase `y`
  - "move down" = decrease `x`
  - "wider" = increase `h`
  - "taller" = increase `w`
  - Use `labelv` for normal horizontal text
- When reading existing controls, the coordinates describe portrait positions
  that the editor rotates

### Quick reference for landscape control placement

| Editor position  | XML coordinate          |
|-----------------|-------------------------|
| Top-left        | high `x`, low `y`       |
| Top-right       | high `x`, high `y`      |
| Bottom-left     | low `x`, low `y`        |
| Bottom-right    | low `x`, high `y`       |
| Full page width | `h` ≈ 1024              |
| Full page height| `w` ≈ 730               |

### Why not transform to editor coordinates?

We tried swapping x↔y and w↔h during parse/serialize to make the model match
the editor. This was abandoned because:

1. The y-axis also needs to be inverted (not just swapped), requiring knowledge
   of the canvas dimensions
2. Multiple iterations of "fix the swap" produced incorrect results
3. Byte-perfect round-trip testing is simpler without transforms
4. The existing fixture layouts parsed from templates work correctly as-is

The tradeoff is that code creating new controls needs to think in rotated
coordinates, but this is documented above and was verified empirically.
