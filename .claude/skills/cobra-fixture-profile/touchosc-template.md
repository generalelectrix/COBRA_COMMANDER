# TouchOSC group template authoring

A `.touchosc` file is a ZIP containing a single `index.xml` (TouchOSC v17 layout). String
attributes (`name`, `osc_cs`, `text`, `li_t`, `la_t`) are **base64-encoded**. It lives at
`touchosc/group_templates/<StructName>.touchosc` and is embedded at compile time by
`#[derive(PatchFixture)]` (use `#[no_touchosc_template]` until it exists).

Generate it with [touchosc_layout.py](touchosc_layout.py): edit a layout function, run the
script to (re)write the file, then rebuild. **Always crib geometry from existing templates**
rather than guessing — unzip `WizardExtreme.touchosc` (color wheel, gobo, rotation knobs,
split-color) and `IWashLed.touchosc` (moving head pan/tilt + positioner presets) and copy.

## The landscape coordinate gotcha (read first)

Also read the repo's `src/touchosc/CLAUDE.md`. Summary: all Cobra layouts use
`orientation="vertical"`, which the editor renders as **landscape** (90° CCW). So in the XML:

- **XML `x`** = the editor's **vertical** axis (and `x` increases **upward**: x=0 bottom, ~730 top).
- **XML `y`** = the editor's **horizontal** axis (y increases rightward, 0…~1024).
- **XML `w`** = the control's **height** in the editor; **XML `h`** = its **width**.
- **`labelv`** reads **left-to-right** in the editor (the one you want); `labelh` reads bottom-to-top.

Mental model used by the generator: "**columns**" are constant-`x` bands stacked along the
vertical (editor) axis; controls within a band spread along `y` (the long/horizontal axis).
Higher `x` band = higher on screen. A full-width fader is `w≈84, h≈1015`.

To order bands top→bottom, assign **decreasing** `x` (top band = highest `x`).

## Control elements

`type="faderv"` (fader; `centered="true"` + `scalef="-1.0"` for bipolar), `type="toggle"`,
`type="push"` (momentary; for `LabeledSelect` slots, `osc_cs = /<Fix>/<ControlName>/<Label>`),
`type="multipush"` (`number_y=N` grid; for `IndexedSelect`, `osc_cs = /<Fix>/<ControlName>`),
`type="labelv"` (static or, with `osc_cs`, a writeable label like positioner presets).

OSC addresses **must match the profile exactly** — `LabeledSelect` label → `/Fix/Color/Red`,
`Mirrored` adds `/Fix/MirrorPan`, `.with_split` adds `/Fix/SplitColor`, positioner adds
`/Fix/PositionPresetSelect` + `/Fix/PositionPresetLabel/0..7`. After generating, decode every
`osc_cs` and diff against the profile's controls.

## Label conventions (learned by iteration)

- **Fader labels**: no background; center along XML `x` (across the fader's thin dimension),
  tuck to the low-`y` end ("off to one side"). Matches iwash.
- **Button labels** (color/gobo names, toggles): **centered in both axes** on the button,
  **with a background** (so they stay legible over a lit button), and the box sized **tight to
  the text** (minimum width — avoid clipping). Make the box's editor-height ≈ `font_size + 12`
  so descenders (g/y/p) aren't cut.
- **Multi-line**: at size 20, word-wrap long names onto stacked lines (separate `labelv`s along
  XML `x`, line 0 on top = highest `x`), each line a tight box. e.g. "asym tri spiral" →
  "asym tri" / "spiral".
- **Per-button names** on a gobo multipush: one centered label per cell (cell height =
  `multipush_h / number_y`); index 0 is usually "open".
- **Color display labels** can be lowercase even though the OSC name (push `osc_cs`) keeps the
  profile's capitalization.
- **Two-tone / litho labels**: e.g. a white-on-blue gobo "pents on blue" → two lines, the
  second tinted `blue`.
- **Draw order = z-order**: later elements draw on top. Emit a label after a sibling to bring
  it to the front (e.g. an arrow over the split-color text). Emit overlay labels *after* the
  multipush so they sit on top of the buttons.

## Cribbing fixed clusters verbatim

Some clusters were tuned by eye in existing templates — copy their exact relative offsets:

- **Positioner preset labels** (writeable, from `IWashLed.touchosc`): per-button `labelv` with
  `osc_cs=/Fix/PositionPresetLabel/i`, `w=25 h=105`, at `y ∈ {15,143,270,396,523,649,776,903}`;
  pair with a `PositionPresetSelect` multipush (`y=3 h=1018 number_y=8`).
- **Split-color indicator** (from `WizardExtreme.touchosc`): an 84×84 `SplitColor` toggle plus
  three labels at relative offsets `color?`(+12,+13, w33 h59), `→`(+26,+19, w33 h43, no bg),
  `split`(+39,+20, w33 h43). The arrow points toward the next color — consistent because
  `.with_split` adds a **positive** offset (toward the next slot). Keep the arrow's direction.

## Verify after generating

Unzip the result and check, by decoding base64: (1) every `osc_cs` matches a profile control;
(2) `multipush number_y` matches each `IndexedSelect`'s `n`; (3) the number of color pushes
matches the `LabeledSelect`; (4) band `x` order is what you intend (top→bottom). Then rebuild —
a successful build means the template embedded, and `test_all_fixtures_handle_declared_controls`
still passing means the OSC namespace is consistent.

TouchOSC's built-in color palette is limited (`gray, red, green, blue, yellow, orange, purple,
pink`) — there's no true cyan/magenta; pick the nearest tint for button accents.
