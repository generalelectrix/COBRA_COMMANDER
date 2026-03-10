# Cobra Commander GUI Framework Selection & Implementation Plan

## Context

Cobra Commander is a Rust DMX lighting controller that currently uses a CLI wizard (`run_cli_configuration` in `src/cli.rs`) for startup configuration: clock source, MIDI devices, DMX port assignment, and animation visualizer launch. The project already uses egui/eframe (`eframe 0.32.3`, `egui_plot 0.33.0`) for a separate animation visualizer subprocess. The goal is to select **a single GUI framework** for building the full application GUI — including both widget-heavy configuration panels AND the animation visualizer (real-time waveform plots) — with cross-platform support (macOS, Windows, Linux), native rendering (not browser-based), and long-term extensibility for significant GUI development ahead.

**Critical constraint**: Two GUI frameworks is unacceptable. The animation visualizer (currently a separate egui subprocess) must be merged into the main GUI application. Whatever framework is chosen must handle both widget-heavy forms AND real-time plotting.

This decision is high-stakes: the chosen framework will be the foundation for a large amount of future GUI work.

---

## Part 1: Rust GUI Framework Landscape (March 2026)

### Framework Overview

| Framework | Stars | Version | API Stable? | Rendering | Accessibility | Widget Richness |
|-----------|-------|---------|-------------|-----------|---------------|-----------------|
| **Slint** | 21.9k | 1.15.1 | **Yes (1.x)** | Skia/FemtoVG/Software | Full | **Comprehensive** |
| **egui** | 28.3k | 0.32+ | No | wgpu/glow (GPU) | Full | Moderate + ecosystem |
| **iced** | 29.8k | 0.14.0 | No | wgpu (GPU) | **None** | Moderate |
| **Dioxus** | 35.2k | 0.7.3 | No | WebView (browser) | Full (web) | Web ecosystem |
| **gtk4-rs** | ~3k | Stable | Yes | Cairo/GL (native) | Broken on Win | **Most complete** |
| **Tauri** | 104k | 2.10.1 | Yes | WebView (browser) | Full (web) | Web ecosystem |
| **Xilem** | 4.9k | 0.4.0 | No (alpha) | Vello (GPU) | Partial | Minimal |
| **Floem** | 4.0k | 0.2.0 | No | wgpu/tiny-skia | **None** | Limited |
| **Freya** | ~2k | pre-1.0 | No | Skia | Partial | Limited |

### Key Findings from 2025 Survey

Of 43 Rust GUI libraries surveyed, **94.4% were not production-ready**. Only Slint, Dioxus, egui, and WinSafe passed all three tests (basic functionality + accessibility + IME). iced and Floem failed accessibility entirely.

### Eliminated Options

- **Dioxus**: Desktop mode = WebView. Violates native preference. IPC boundary loses type safety.
- **Tauri**: Web UI, not native. Explicitly unwanted.
- **gtk4-rs**: Looks alien on macOS/Windows. Screen reader broken on Windows. GObject fights Rust idioms. Not thread-safe (`!Send`, `!Sync`).
- **Xilem**: Alpha state, minimal widgets, no docs. Check back in 12-18 months.
- **Floem**: No accessibility, no IME, 2 releases ever, built for Lapce only.
- **Freya**: Too immature, partial accessibility only.

### Top 3 Candidates

**1. Slint** — Declarative DSL, stable 1.x API, commercial backing, best widget library
**2. egui** — Already in use, immediate mode, great testability, fast prototyping
**3. iced** — Elm architecture, clean state management, but no accessibility and incomplete docs

---

## Part 1B: Plotting & Real-Time Visualization Capability

**This section was added after the initial research to address a critical constraint: the animation visualizer must be merged into the main GUI. The chosen framework must support real-time waveform plotting.**

### What the Animation Visualizer Needs

The current visualizer (`src/animation_visualizer.rs`, 233 lines) uses `egui_plot` to render:
- **2 line plots** (1000 data points each): unit waveform (dark red) and scaled waveform (white)
- **1 scatter plot**: fixture values as cyan dots (one per fixture)
- **Axes**: X = phase (0.0–1.0), Y = amplitude (-1.0–1.0)
- **Interactivity**: pan, zoom, hover tooltips (built-in from egui_plot)
- **Real-time updates**: ZMQ pub/sub feed, repaints on data arrival (~25ms intervals)
- **Complexity**: Low-moderate. Simple overlay of 2-3 time-series.

### Framework Plotting Capabilities

| Capability | egui | Slint | iced |
|-----------|------|-------|------|
| Built-in plot widget | **egui_plot** (Line, Points, axes, zoom, pan, hover) | **None** | **None** (but `plotters-iced` backend exists) |
| Canvas/custom drawing | **Excellent** (painter API) | **No imperative canvas** — only declarative `Path` element | Canvas widget available |
| Third-party plotting | egui_plot (first-class) | **Zero crates** on crates.io | `plotters-iced` (maintained) |
| wgpu integration | Native (egui renders via wgpu) | `slint::wgpu_28` — render to texture, import as Image | Native (iced renders via wgpu) |
| Real-time 40fps plots | **Trivial** | Possible via wgpu texture route only | Possible via canvas or plotters-iced |
| Scatter plot support | Built-in (Points widget) | Must draw individual rectangles/circles | Via plotters-iced or custom canvas |
| Interactive zoom/pan | Built-in | Must build from scratch | Must build from scratch |

### Slint Plotting Deep Dive

Researched extensively. Key findings (sourced from Slint GitHub discussions, co-founder statements, community projects):

1. **No built-in plot widget**. Confirmed by co-founder @ogoffart in Discussion #9518. No roadmap to add one.
2. **No imperative canvas API**. Only the declarative `Path` element with SVG commands. A Canvas API has been discussed but not shipped.
3. **Zero third-party Slint plotting crates** on crates.io. No `plotters-slint` backend.
4. **Path element** can draw data-driven lines via SVG command strings, but: no axes, no labels, no interaction (zoom/pan/hover), questionable performance at 25ms refresh with 1000 points, no scatter plot primitive.
5. **wgpu texture rendering** (Slint 1.12+) is the viable high-performance path — but means writing your own GPU plot renderer, which is exactly what egui_plot already does.
6. **plotters-to-bitmap** works (official example exists) but is CPU-bound, loses vector crispness, and plotters is better suited for static charts than real-time animation.
7. **No egui embedding in Slint**. Architecturally incompatible. No integration crate exists.
8. **Community consensus**: Everyone building plots in Slint converges on "render externally, display as Image."

**Initial verdict was that Slint cannot replace egui_plot.** However, further research revealed a viable path:

### The plotters → Slint Image Path (Changes Everything)

Slint maintains an **official example** (`examples/plotter`) demonstrating real-time chart rendering via `plotters` → `SharedPixelBuffer` → `Image::from_rgb8()`. The integration is 3 API calls with zero copies:

```rust
let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(800, 400);
let backend = BitMapBackend::with_buffer(pixel_buffer.make_mut_bytes(), (800, 400));
// ... plotters renders charts directly into the buffer ...
let image = Image::from_rgb8(pixel_buffer);  // zero copy
app.set_plot_image(image);
```

**Key findings:**
- **~75-100 lines of Rust** to build the full plotter integration (line charts, scatter, axes, colors)
- **Performance**: plotters renders 2×1000 line segments + scatter in ~2-5ms on CPU. Budget is 25ms. That's 5-12× headroom.
- **Pure CPU, zero cross-platform risk**: No GPU, no shaders, no platform-specific code.
- **plotters has all features**: `LineSeries`, `Circle` (scatter), custom colors/widths, axis labels, grid lines — all built-in.
- **Official Slint precedent**: The `examples/plotter` demo validates this exact pattern.

**What you lose vs egui_plot**: Built-in zoom/pan/hover interaction. For the animation visualizer (fixed axes, display-only), this is acceptable. If interactive exploration is needed later, mouse event handling can be added (~50 additional lines).

**What you gain**: Clean .slint DSL for UI, stable 1.x API, platform-adaptive themes, comprehensive widget library — all the Slint advantages with plotting solved.

### Revised impact on ranking

- **Slint returns to #1** — the plotters integration path makes it viable as a single-framework solution with ~100 lines of plotter code
- **egui remains strong at #2** — plotting is more convenient (first-class), but the other trade-offs (API instability, event loop complexity, developer-tool aesthetic) remain
- **iced stays #3** — `plotters-iced` exists but accessibility/docs issues remain

---

## Part 2: Implementation Plans

### What the GUI Replaces

`run_cli_configuration()` in `src/cli.rs:92-102` — 4 sequential configuration steps:

1. **Clock**: Remote clock service (browse/select provider) OR local audio device
2. **MIDI**: Auto-detect devices, confirm or manually select
3. **DMX Ports**: Optional ArtNet scan, per-universe port assignment (indexed, 0=offline)
4. **Visualizer**: Launch the egui animation visualizer

All commands flow through `CommandClient.send_command(MetaCommand::*)` to the running show. The CLI runs on a separate thread; the show loop runs at 25.3ms intervals.

---

### Plan A: Slint

**Architecture**: Slint event loop on main thread. Show loop on spawned thread. Communication via existing `CommandClient` (Clone + Send). Background discovery threads use `slint::invoke_from_event_loop()` to push results back to UI.

**Threading model**:
```
Main thread:  slint::run_event_loop()
Show thread:  show.run() (spawned)
Discovery:    short-lived threads for ArtNet scan, MIDI list, clock browse
```

**Widget mapping**:
| CLI Step | Slint Widgets |
|----------|--------------|
| Clock source selection | `RadioButton` (remote vs local) |
| Provider browsing | `StandardListView` + "Browse" `Button` |
| Audio device selection | `StandardListView` |
| MIDI auto-detect display | Read-only `StandardListView` |
| MIDI manual config | `ComboBox` for device type + ports |
| ArtNet scan toggle | `CheckBox` + "Scan" `Button` |
| Per-universe port assignment | `ComboBox` per universe (in vertical layout) |
| Visualizer launch | Single `Button` |

**Data flow**: UI callbacks fire on main thread → spawn worker thread with `CommandClient` clone → `send_command(MetaCommand::*)` → `invoke_from_event_loop()` to update status. Discovery results pushed to `VecModel<T>` via `invoke_from_event_loop()`.

**File structure**:
```
build.rs                    (5 lines)
ui/app.slint                (~80 lines — main window, tab navigation)
ui/clock_config.slint       (~60 lines)
ui/midi_config.slint        (~80 lines)
ui/dmx_config.slint         (~70 lines)
ui/visualizer.slint         (~20 lines)
ui/common.slint             (~30 lines — shared styles)
src/gui/mod.rs              (~50 lines — entry point)
src/gui/bridge.rs           (~300 lines — callback wiring, CommandClient calls)
src/gui/discovery.rs        (~150 lines — background scanning)
```

**Dependencies**: `slint = "1.15"` + `slint-build = "1.15"` (build-dep). Existing eframe/egui kept for visualizer.

**Estimated size**: ~850 lines Rust + ~340 lines .slint DSL = **~1,200 lines total**

**Migration**: Add `--gui` flag → Phase 1: empty window → Phase 2: one panel at a time → Phase 3: wire to show → Phase 4: make default.

---

### Plan B: egui

**Architecture**: **Separate process** (same pattern as existing animation_visualizer). Config GUI spawns as `Command::new(binary).arg("config").spawn()`. Communicates with show via ZMQ REQ/REP protocol. This avoids the macOS winit event loop ownership problem entirely.

**Threading model**:
```
Show process:
  Main thread:  show.run()
  Bridge thread: ZMQ REP server (translates ConfigRequest → MetaCommand)

GUI process (separate):
  Main thread:  eframe::run_native()
  ZMQ thread:   sends ConfigRequest, receives ConfigResponse
```

**Widget mapping**:
| CLI Step | egui Widgets |
|----------|-------------|
| Clock source selection | `ComboBox` (remote vs local) |
| Provider browsing | `ComboBox` (populated async) |
| Audio device selection | `ComboBox` |
| MIDI auto-detect display | Labeled list in `ui.group()` |
| MIDI manual config | `ComboBox` for device type + ports, collapsing section |
| ArtNet scan toggle | `Checkbox` + "Scan Now" `Button` |
| Per-universe port assignment | `Grid` with `ComboBox` per row |
| Visualizer launch | Single `Button` |

**Data flow**: User interaction → `ConfigApp::update()` → serialize `ConfigRequest` → ZMQ REQ → show's bridge thread → `CommandClient::send_command()` → `CommandResponse` → ZMQ REP → deserialize → update UI state.

**Critical constraint**: `Box<dyn DmxPort>` cannot cross process boundaries. The show-side bridge must hold discovered ports and accept port indices from the GUI, not port objects. This adds a protocol translation layer that doesn't exist in the CLI.

**File structure**:
```
src/config_gui/mod.rs           (~40 lines)
src/config_gui/app.rs           (~200 lines — eframe::App impl)
src/config_gui/clock_panel.rs   (~120 lines)
src/config_gui/midi_panel.rs    (~180 lines)
src/config_gui/dmx_panel.rs     (~150 lines)
src/config_gui/viz_panel.rs     (~30 lines)
src/config_gui/protocol.rs      (~120 lines — ConfigRequest/Response serde types)
src/config_gui/state.rs         (~60 lines)
src/config_bridge.rs            (~150 lines — ZMQ REP server)
```

**Dependencies**: None new for GUI (eframe already present). Add `serde_json` or `rmp-serde` for protocol. `egui_kittest = "0.32"` for tests.

**Estimated size**: **~1,310 lines** (9 new files, 3 modified)

**Migration**: Add `Command::Config` subcommand → Phase 1: protocol + bridge → Phase 2: scaffold GUI → Phase 3: panels one-by-one → Phase 4: optionally replace CLI.

---

### Plan C: iced

**Architecture**: iced on main thread via `iced::application().run()`. Show loop on spawned thread. Communication via `CommandClient` through `Task::perform` (iced's async task system). Blocking `send_command` calls wrapped in tasks to avoid blocking the UI.

**Threading model**:
```
Main thread:  iced::application().run()
Show thread:  show.run() (spawned)
Async tasks:  Task::perform for send_command, port scanning, etc.
```

**Widget mapping**:
| CLI Step | iced Widgets |
|----------|-------------|
| Clock source selection | `radio` buttons |
| Provider browsing | `pick_list` (populated async via Task) |
| Audio device selection | `pick_list` |
| MIDI auto-detect display | `column` of `text` items |
| MIDI manual config | `pick_list` for type + ports |
| ArtNet scan toggle | `toggler` + "Scan" `button` |
| Per-universe port assignment | `pick_list` per universe in `column` |
| Visualizer launch | Single `button` |

**Data flow (Elm architecture)**:
```
User action → Message variant → update() → Task::perform(async { send_command() }) → Message::Result → update() → view()
```

**State**: Single `App` struct holds all state. `Message` enum with ~25 variants covers all interactions. `update()` returns `Task<Message>` for async operations. `view()` renders based on current state.

**File structure**:
```
src/gui/mod.rs              (~30 lines)
src/gui/app.rs              (~300 lines — Message enum, update, view dispatch)
src/gui/clock_panel.rs      (~120 lines)
src/gui/midi_panel.rs       (~180 lines)
src/gui/dmx_panel.rs        (~150 lines)
src/gui/visualizer_panel.rs (~40 lines)
src/gui/style.rs            (~60 lines)
src/gui/subscription.rs     (~50 lines)
```

**Dependencies**: `iced = { version = "0.14", features = ["tokio"] }`. Existing eframe kept for visualizer (separate process, no conflict).

**Estimated size**: **~930 lines** (7 new files, 2 modified)

**Migration**: Same `--gui` flag approach. Show moves to spawned thread when GUI mode active.

---

## Part 3: Comparative Analysis

### Complexity

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| New lines of code | ~1,200 | ~1,310 | ~930 |
| New files | 10 | 9 | 7 |
| New languages | .slint DSL | None | None |
| Build system changes | build.rs required | None | None |
| Protocol/IPC layer | None | **ZMQ REQ/REP protocol** | None |
| Threading complexity | Low (invoke_from_event_loop) | Medium (separate process + ZMQ) | Low (Task::perform) |

**Verdict**: iced is simplest in raw LOC. egui is most complex due to the separate-process + protocol layer. Slint adds the .slint DSL but the Rust side is clean.

### Maintainability

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| API stability | **Stable 1.x semver** | Breaking changes every release | Breaking changes every release |
| Upgrade friction | Low (semver) | High (API churn) | High (API churn) |
| Commercial backing | **SixtyFPS GmbH (full-time devs)** | Community + Rerun sponsorship | Community |
| Documentation quality | **Good (book + API docs)** | Good (examples, API docs) | Poor (book incomplete, "wait patiently") |
| Separation of concerns | **Clean (UI markup vs logic)** | Mixed (UI in Rust code) | Clean (Model/View/Update) |

**Verdict**: Slint wins decisively. A stable API means you don't rewrite GUI code every time you update. Commercial backing means it won't be abandoned. The .slint/Rust separation keeps UI layout out of business logic.

### Look and Feel

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| Default appearance | **Platform-adaptive** (Fluent/Cupertino/Material) | Developer-tool aesthetic | Custom-rendered, polished |
| Native feel | **Best** (adapts per OS) | Worst (looks like ImGui) | Middle (consistent but not native) |
| Styling system | Declarative properties + built-in themes | Programmatic Visuals struct | Theme trait + per-widget .style() |
| Visual polish achievable | High with low effort | Medium with high effort | High with medium effort |
| Live preview | **VS Code extension with drag-drop** | None | None |

**Verdict**: Slint looks best out of the box and requires the least effort to look professional. egui looks like a dev tool without significant work. iced can look good but requires custom theming.

### Extensibility

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| Built-in widgets | **Most comprehensive** (Table, ListView, GridView, TabWidget, Dialog, etc.) | Moderate (needs egui_extras, third-party crates) | Moderate (pick_list, scrollable, no tree view) |
| Custom widgets | .slint components (declarative) | Immediate-mode (procedural) | Custom Widget trait (retained) |
| Complex UI patterns | TabWidget, Dialog, PopupWindow built-in | egui_dock, egui-notify (third-party) | iced_aw for tabs, menus |
| Canvas/custom drawing | Possible but not primary use case | **Excellent** (painter API, egui_plot) | Canvas widget available |
| Future GUI needs (cue editor, fixture table, DMX inspector) | Well-served by StandardTableView, ListView, GridView | Would need custom widgets or third-party | Possible but more custom work |

**Verdict**: For a DMX controller that will eventually need tables, lists, complex panels — Slint's built-in widget set is the strongest foundation. egui excels at visualization/canvas work (which you already use it for). iced is in between.

### Testability

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| First-class test framework | Screenshot testing, CI-compatible | **egui_kittest** (best: simulated clicks, snapshot testing) | **None** |
| Headless mode | Software renderer | wgpu-based harness | No |
| Unit testing business logic | Callbacks are Rust functions | Panel methods are Rust functions | **update() is a pure function** (best for unit testing) |
| Integration testing | Moderate | Good (Harness API) | Poor |

**Verdict**: egui has the most mature GUI testing story with egui_kittest. iced's Elm architecture makes business logic unit-testable (update is a pure function), but has no GUI testing. Slint has screenshot testing but less mature than egui_kittest. This is a genuine advantage for egui.

### Integration with Cobra Commander

| Dimension | Slint | egui | iced |
|-----------|-------|------|------|
| Event loop coexistence | **Best**: `invoke_from_event_loop()` designed for this | Worst: separate process needed on macOS | Good: `Task::perform` for async |
| Show loop threading | Show on spawned thread, Slint on main | Show on main (unchanged), GUI is separate process | Show on spawned thread, iced on main |
| CommandClient integration | Direct call from spawned worker threads | ZMQ protocol layer required | `Task::perform` wraps blocking calls |
| `Box<dyn DmxPort>` handling | `Arc<Mutex<Vec<Option<...>>>>` + index | Cannot cross process boundary — protocol indirection | Held in App state, moved on assignment |
| Existing egui coexistence | No conflict (different windows) | Same crate, but visualizer is separate process anyway | No conflict (different processes) |

**Verdict**: Slint has the cleanest integration story — it was designed for apps with their own event loops. egui's separate-process requirement adds the most architectural complexity (ZMQ protocol, serialization, port indirection). iced is clean but requires moving the show to a spawned thread.

---

## Part 4: Risk Analysis

### Slint Risks
- **.slint DSL learning curve**: New language, but simple and well-documented. VS Code extension helps.
- **Smaller community**: Fewer Stack Overflow answers, fewer third-party crates. Mitigated by comprehensive built-in widgets.
- **Clean build times**: Compiles everything from source. Incremental builds are fast.
- **Licensing**: Royalty-free license covers desktop apps. No issue for Cobra Commander.

### egui Risks
- **Separate process architecture**: Most complex of the three. ZMQ protocol, serialization, port indirection.
- **API instability**: Every egui release breaks things. Constant maintenance cost.
- **Event loop constraint**: The macOS limitation forces the subprocess model, which adds permanent architectural complexity.
- **Widget gaps for future needs**: Tables, tree views, advanced lists all require third-party crates of varying quality.

### iced Risks
- **No accessibility**: Screen readers don't work. If accessibility ever matters, you'd have to switch frameworks.
- **Incomplete documentation**: "Wait patiently until the book is finished" is a direct quote from iced docs.
- **Still experimental after 6 years**: No 1.0 in sight. API continues to churn.
- **Box<dyn DmxPort> ownership**: iced's Message must be Clone, but DmxPort is not Clone. Requires holding ports in the Model and passing indices through Messages. Workable but adds friction for every non-Clone type.

---

## Part 5: Revised Final Ranking

**The single-framework constraint is satisfied by all three top candidates**, but through different mechanisms. Slint uses plotters→bitmap→Image (official example, ~100 lines). egui uses egui_plot (built-in). iced uses plotters-iced.

### #1: Slint (Recommended)

**Why it wins**:
- **Stable API** — the single most important factor for extensive GUI development. 1.x semver. No rewriting code on every update.
- **Best widget library** — StandardTableView, ListView, GridView, ComboBox, TabWidget, Dialog all built-in. Future features (cue editor, fixture manager, DMX inspector) well-served.
- **Cleanest integration** — `invoke_from_event_loop()` purpose-built for apps with their own event loops.
- **Best default appearance** — platform-adaptive themes (Fluent/Cupertino) with minimal effort. A lighting controller benefits from looking polished.
- **Commercial backing** — full-time developers, clear roadmap, no abandonment risk.
- **Plotting solved** — plotters→SharedPixelBuffer→Image::from_rgb8() is ~100 lines, officially supported by Slint examples, pure CPU (zero cross-platform risk), 5-12× performance headroom.
- **Clean separation** — .slint DSL for layout, Rust for logic. More readable and maintainable long-term.

**Trade-offs accepted**:
- **.slint DSL** is a second language, but simple and well-documented. VS Code extension with live preview.
- **Plotting is bitmap-based**, not interactive vector. No built-in zoom/pan/hover. For the animation visualizer (fixed axes, display-only), this is fine. Interactive exploration can be added later via mouse event handling (~50 lines).
- **Smaller community** than egui. Mitigated by comprehensive built-in widgets needing fewer third-party crates.

### #2: egui/eframe (Strong Alternative)

**Why it's #2**:
- **Plotting is first-class** — `egui_plot` with zoom/pan/hover built-in. Most convenient plotting story.
- **Already in use** — lowest friction to start.
- **Best testability** — `egui_kittest` for simulated interactions and snapshot testing.

**Why it doesn't win**:
- **API instability** — breaking changes every release. Ongoing maintenance tax.
- **Developer-tool aesthetic** — looks like ImGui without significant custom styling work.
- **Event loop ownership on macOS** — Show must be restructured to spawned thread. Show contains non-Send types, requiring construction on the worker thread.
- **Widget gaps grow with ambition** — tables, tree views, docking all need third-party crates.
- **No separation of concerns** — UI layout mixed into Rust code.

### #3: iced (Not Recommended)

**Why it drops**: Zero accessibility. Incomplete documentation ("wait patiently"). Six years without 1.0. The Elm architecture is elegant but practical deficits outweigh it.

---

## Part 6: Recommended Implementation Plan (Slint)

### Architecture

Slint event loop on main thread. Show loop on spawned thread. Communication via existing `CommandClient` (Clone + Send). Background discovery threads use `slint::invoke_from_event_loop()` to push results back to UI. Animation visualizer rendered via `plotters` → `SharedPixelBuffer` → `Image::from_rgb8()` on a `slint::Timer` at 25ms intervals.

**Threading model:**
```
Main thread:  slint::run_event_loop() — Slint owns the event loop
Show thread:  show.run() (spawned) — 25.3ms control/update/render/write loop
Discovery:    short-lived threads for ArtNet scan, MIDI list, clock browse
```

The show currently runs on the main thread. With `--gui`, it must move to a spawned thread so Slint can take the main thread (required on macOS). `CommandClient` is `Clone + Send` and works cross-thread.

**Key constraint**: `Show` contains non-Send types (trait objects, Rc, etc.). It must be **constructed** on the worker thread, not moved there after construction. This means `run_show()` restructures: when `--gui`, the worker thread builds and runs the Show, while main thread builds and runs the Slint app.

**Animation data flow**: The existing `AnimationPublisher` publishes `AnimationServiceState` via `Arc<Mutex<>>`. The Slint timer reads this shared state, renders via plotters into a `SharedPixelBuffer`, and sets the resulting `Image` on the Slint component. No ZMQ needed when in-process.

### File structure

```
build.rs                    (5 lines — slint_build::compile)
ui/
  app.slint                 (~100 lines — main window, tab navigation)
  clock_config.slint        (~60 lines)
  midi_config.slint         (~80 lines)
  dmx_config.slint          (~70 lines)
  visualizer.slint          (~30 lines — Image element for plotters output)
  common.slint              (~30 lines — shared styles)
src/gui/
  mod.rs                    (~60 lines — entry point, run_gui())
  bridge.rs                 (~300 lines — callback wiring, CommandClient calls)
  discovery.rs              (~150 lines — background scanning threads)
  plotter.rs                (~100 lines — plotters→SharedPixelBuffer→Image rendering)
```

### Widget mapping

| CLI Step | Slint Widgets |
|----------|--------------|
| Clock source selection | `RadioButton` (remote vs local) |
| Provider browsing | `StandardListView` + "Browse" `Button` |
| Audio device selection | `StandardListView` |
| MIDI auto-detect display | Read-only `StandardListView` |
| MIDI manual config | `ComboBox` for device type + ports |
| ArtNet scan toggle | `CheckBox` + "Scan" `Button` |
| Per-universe port assignment | `ComboBox` per universe (in vertical layout) |
| Animation visualizer | `Image` element displaying plotters-rendered bitmap |

### Animation visualizer via plotters

The core rendering function (~75 lines):

```rust
use plotters::prelude::*;
use slint::{SharedPixelBuffer, Rgb8Pixel, Image};

fn render_animation_plot(state: &AnimationServiceState) -> Image {
    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(800, 400);
    {
        let backend = BitMapBackend::with_buffer(
            buffer.make_mut_bytes(), (800, 400)
        );
        let root = backend.into_drawing_area();
        root.fill(&RGBColor(30, 30, 30)).unwrap(); // dark background

        let mut chart = ChartBuilder::on(&root)
            .build_cartesian_2d(0.0f64..1.0, -1.0f64..1.0)
            .unwrap();

        // Unit waveform (dark red, 1000 points)
        chart.draw_series(LineSeries::new(
            (0..1000).map(|i| {
                let phase = i as f64 / 999.0;
                (phase, state.animation.sample(phase))
            }),
            RGBColor(139, 0, 0).stroke_width(2),
        )).unwrap();

        // Scaled waveform (white, 1000 points)
        chart.draw_series(LineSeries::new(
            (0..1000).map(|i| {
                let phase = i as f64 / 999.0;
                (phase, state.animation.sample(phase) * state.audio_envelope)
            }),
            WHITE.stroke_width(2),
        )).unwrap();

        // Fixture values (cyan scatter)
        chart.draw_series(
            (0..state.fixture_count).map(|i| {
                let phase = /* fixture phase offset */;
                let value = /* sampled value */;
                Circle::new((phase, value), 5, CYAN.filled())
            })
        ).unwrap();
    }
    Image::from_rgb8(buffer)
}
```

Wired to Slint via a timer:
```rust
let timer = slint::Timer::default();
let weak = window.as_weak();
let animation_state = animation_state.clone(); // Arc<Mutex<AnimationServiceState>>
timer.start(slint::TimerMode::Repeated, Duration::from_millis(25), move || {
    if let Some(window) = weak.upgrade() {
        let state = animation_state.lock().unwrap();
        window.set_plot_image(render_animation_plot(&state));
    }
});
```

In `ui/visualizer.slint`:
```slint
export component VisualizerPanel inherits VerticalLayout {
    in property <image> plot_image;
    Image {
        source: plot_image;
        width: 100%;
        height: 100%;
        image-fit: contain;
    }
}
```

**Performance**: plotters renders 2×1000 line segments + 20 scatter circles in ~2-5ms on CPU. Budget is 25ms. 5-12× headroom. Pure CPU, zero cross-platform risk.

**What you lose vs egui_plot**: Built-in zoom/pan/hover. For the animation visualizer (fixed axes, display-only), this is fine. Interactive exploration can be added later (~50 lines of mouse event handling).

### Data flow

**Config commands (UI → Show):**
```
User clicks button in Slint → callback fires on main thread →
  spawn worker thread with CommandClient clone →
  send_command(MetaCommand::*) → Show processes → response →
  invoke_from_event_loop() → update Slint status text
```

**Device discovery (background → UI):**
```
spawn thread → rust_dmx::available_ports() or tunnels::midi::list_ports() →
  invoke_from_event_loop() → push results to VecModel<T> → UI updates
```

**Animation data (Show → UI):**
```
Show updates Arc<Mutex<AnimationServiceState>> →
  Slint Timer (25ms) reads shared state →
  plotters renders to SharedPixelBuffer →
  set_plot_image(Image::from_rgb8(buffer))
```

### Dependencies

```toml
[dependencies]
slint = "1.15"
plotters = "0.3"  # for animation visualizer rendering

[build-dependencies]
slint-build = "1.15"
```

Existing `eframe` and `egui_plot` deps can be removed once the standalone visualizer is deprecated (Phase 5).

### Estimated size

**~990 lines Rust + ~370 lines .slint DSL = ~1,360 lines total** across 11 new files, 2-3 modified files. The visualizer panel is ~100 lines of plotters rendering code replacing the 233-line animation_visualizer.rs.

### Migration phases

**Phase 1: Foundation + thread restructuring (1 session)**
- Add `slint = "1.15"` and `slint-build = "1.15"` to Cargo.toml
- Create `build.rs` to compile `.slint` files
- Add `--gui` flag to `RunArgs`
- Restructure `main.rs`: when `--gui`, build Show on worker thread, run Slint on main
- Create `ui/app.slint` with minimal tabbed window + `src/gui/mod.rs`
- Verify: window opens, show still runs, `--quickstart` unaffected

**Phase 2: Animation visualizer panel (1 session)**
- Add `plotters = "0.3"` to Cargo.toml
- Create `src/gui/plotter.rs` with `render_animation_plot()` function
- Create `ui/visualizer.slint` with Image element
- Wire `slint::Timer` to read `Arc<Mutex<AnimationServiceState>>` and render
- Verify: waveform plots render in the GUI tab, same visual output as standalone `viz` command

**Phase 3: DMX panel (1 session)**
- Implement `ui/dmx_config.slint` — ComboBox per universe
- Implement port discovery in `src/gui/discovery.rs`
- Wire callbacks in `src/gui/bridge.rs` — `AssignDmxPort` via CommandClient
- `Box<dyn DmxPort>` held in `Arc<Mutex<Vec<Option<Box<dyn DmxPort>>>>>`, GUI picks by index

**Phase 4: Clock + MIDI panels (1 session)**
- Implement `ui/clock_config.slint` — RadioButtons + provider ListView
- Implement `ui/midi_config.slint` — auto-detect display + manual ComboBoxes
- Wire `UseClockService`, `SetAudioDevice`, `AddMidiDevice`, `ClearMidiDevice`

**Phase 5: Polish + deprecate standalone visualizer (1 session)**
- Status indicators, error display, configuration-complete state
- Remove `Command::Viz` subcommand and `animation_visualizer.rs`
- Remove `eframe` and `egui_plot` dependencies
- Make `--gui` the default, add `--cli` fallback
- Full end-to-end testing

### Critical files to modify
- `/Users/macklin/src/COBRA_COMMANDER/Cargo.toml` — add slint, plotters; eventually remove eframe/egui_plot
- `/Users/macklin/src/COBRA_COMMANDER/src/main.rs` — add `mod gui`, `--gui` flag, thread restructuring
- `/Users/macklin/src/COBRA_COMMANDER/src/cli.rs` — add `gui: bool` to RunArgs

### Critical files to reference
- `/Users/macklin/src/COBRA_COMMANDER/src/cli.rs` — all 4 config steps to replicate
- `/Users/macklin/src/COBRA_COMMANDER/src/control.rs` — CommandClient, MetaCommand, CommandResponse
- `/Users/macklin/src/COBRA_COMMANDER/src/clock_service.rs` — clock provider browsing logic
- `/Users/macklin/src/COBRA_COMMANDER/src/animation_visualizer.rs` — visualizer logic to port (data feed, colors, series)
- `/Users/macklin/src/COBRA_COMMANDER/src/show.rs` — Show construction, AnimationPublisher, AnimationServiceState

### Verification
1. `cargo build` with slint + plotters compiles clean
2. `cargo run -- run --gui patch/default.yaml` opens Slint window with tabs
3. Animation visualizer tab shows live waveform plots (same visual output as standalone `viz`)
4. Each config panel sends correct MetaCommand when user interacts
5. Show loop continues at 25.3ms while GUI is open
6. `--quickstart` still bypasses both GUI and CLI
7. CLI fallback (`cargo run -- run patch/default.yaml` without `--gui`) works unchanged
8. Standalone `viz` command still works during transition (removed in Phase 5)
