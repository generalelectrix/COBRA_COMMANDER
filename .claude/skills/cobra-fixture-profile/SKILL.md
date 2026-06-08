---
name: cobra-fixture-profile
description: Author a new Cobra Commander DMX fixture profile from a manufacturer manual, plus its TouchOSC group template. USE WHEN adding a fixture profile, writing a moving-head/wash/spot profile, reading a fixture DMX chart/manual, or building/editing a .touchosc group template for a fixture.
---

# Cobra Commander fixture profile authoring

Cobra Commander is an improvisation-first ("lighting jazz") DMX controller. A fixture
**profile** is a Rust struct in `src/fixture/profile/<name>.rs` that maps live-controllable
parameters onto DMX channels; an optional **TouchOSC template** in
`touchosc/group_templates/<StructName>.touchosc` gives it an iPad control surface.

The guiding principle: **expose only what a performer varies live during a set; pin or drop
everything else.** Macros, auto-programs, sound modes, resets, and built-in strobe are not
"lighting jazz" — they get hardcoded to a safe value.

## Workflow

1. **Read the manual.** Manuals are almost always scanned image PDFs — `pdftotext` returns
   nothing. Render pages to images and read them visually:
   ```
   pdftoppm -r 150 -png "manual.pdf" /tmp/manual/page   # needs poppler-utils
   ```
   Then Read the `page-N.png` files. Transcribe the DMX channel chart (channel → function →
   value ranges). Note the channel count / personality (e.g. "15CH").

2. **Decide what to expose vs pin.** Apply the idiom in [control-reference.md](control-reference.md)
   channel by channel. When you can't tell whether a feature belongs, ask. Look at neighboring
   profiles (`wizard_extreme.rs`, `iwash_led.rs`, `rush_wizard.rs`, `astroscan.rs`) for the
   closest precedent.

3. **Write the profile.** Copy [profile-skeleton.rs](profile-skeleton.rs), rename the struct,
   set `#[channel_count = N]`, choose controls, fill `Default` with channel offsets, and write
   `render_with_animations`. Add `pub mod <name>;` to `src/fixture/profile/mod.rs` (the
   `#[derive(PatchFixture)]` macro auto-registers everything else).

4. **Build & test.** Every declared control is fuzzed by an existing test — run it:
   ```
   export PATH="$HOME/.rustup/toolchains/stable-aarch64-unknown-linux-gnu/bin:$HOME/.cargo/bin:$PATH"
   CARGO_TARGET_DIR=/tmp/cobra-target RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build -j 2
   CARGO_TARGET_DIR=/tmp/cobra-target RUSTFLAGS="-C link-arg=-fuse-ld=lld" \
     cargo test -j 2 --bin cobra_commander fixtures_handle
   ```
   `test_all_fixtures_handle_declared_controls ... ok` means the profile is wired correctly.
   (lld is mandatory — the default linker gets OOM-killed in the VM.)

5. **Author the TouchOSC template.** See [touchosc-template.md](touchosc-template.md). Use the
   helper library [touchosc_layout.py](touchosc_layout.py) (which ends with the LilChonker
   layout as a worked example) — edit a layout function, run it to (re)write the `.touchosc`
   file, then rebuild (the file is embedded at compile time by `#[derive(PatchFixture)]`; use
   `#[no_touchosc_template]` until the file exists).

6. **Verify on hardware — the manual lies.** Channel order, wheel slot counts, color/gobo
   DMX values, and "open" positions are frequently wrong in the manual. Confirm against the
   actual head and patch one onto a universe. See the "manual is wrong" notes in
   control-reference.md. Leave generic placeholder names/values where the owner will map them.

## Files

- [control-reference.md](control-reference.md) — feature inclusion idiom + control-type cheatsheet.
- [profile-skeleton.rs](profile-skeleton.rs) — annotated profile skeleton to copy.
- [touchosc-template.md](touchosc-template.md) — template authoring guide (the landscape
  coordinate gotcha, label conventions, cribbing from existing templates).
- [touchosc_layout.py](touchosc_layout.py) — reusable `.touchosc` generator helpers + worked example.

Also read the repo's own `src/touchosc/CLAUDE.md` (coordinate system) and `CLAUDE.md`
(no-panics rule) before editing.
