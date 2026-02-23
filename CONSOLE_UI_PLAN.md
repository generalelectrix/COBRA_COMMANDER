# Plan: COBRA_COMMANDER GUI Console

## Context

The show is currently configured via YAML files and controlled entirely through OSC/MIDI. There is no way to modify the fixture patch, manage OSC/MIDI devices, or configure the show while it is running without editing files and restarting. The goal is to add an `egui`-based GUI console (activated with `--console` on the `run` subcommand) that allows:

- Viewing and editing the fixture patch live (using the existing `repatch()` hot-reload)
- Adding/removing OSC clients at runtime
- Viewing connected MIDI devices and triggering rescan
- Seeing a read-only view of the current patch with DMX addresses and channel assignments

egui/eframe is already a dependency. The `animation_visualizer.rs` establishes the in-process `Arc<Mutex<>>` + `eframe::run_native` pattern to follow.

---

## Architecture

**Threading model**: macOS requires the GUI on the main thread. The Show loop runs in a background thread. They communicate via:
- `Arc<Mutex<ConsoleState>>` — Show writes a snapshot each frame; GUI reads it
- `std::sync::mpsc::channel` — GUI sends `ConsoleCommand` values; Show drains them with `try_recv`

This mirrors how `AnimationVisualizer` shares state today, just in-process rather than via ZeroMQ.

---

## New Files

### `src/console/mod.rs`
Top-level module. Defines `ConsoleHandle` (Show side) and `ConsoleAppHandle` (GUI side). Implements `run_console()` which calls `eframe::run_native`.

```rust
pub struct ConsoleHandle {
    pub state: Arc<Mutex<ConsoleState>>,
    pub commands: Receiver<ConsoleCommand>,
}
pub struct ConsoleAppHandle {
    pub state: Arc<Mutex<ConsoleState>>,
    pub commands: Sender<ConsoleCommand>,
}
pub fn run_console(handle: ConsoleAppHandle) -> Result<()>
```

### `src/console/state.rs`
Snapshot structs written by Show, read by GUI. All `Clone`.

```rust
pub struct ConsoleState {
    pub fixture_types: Vec<FixtureTypeMeta>,  // from PATCHERS, populated once
    pub groups: Vec<GroupSummary>,
    pub osc_clients: Vec<SocketAddr>,
    pub midi_inputs: Vec<String>,
    pub last_error: Option<String>,
}
pub struct FixtureTypeMeta {
    pub name: String,
    pub group_options: Vec<(String, PatchOption)>,
    pub patch_options: Vec<(String, PatchOption)>,
}
pub struct GroupSummary {
    pub key: String,
    pub fixture_type: String,
    pub channel: bool,
    pub color_organ: bool,
    pub options: Vec<(String, String)>,  // group-level key/value
    pub patches: Vec<PatchSummary>,
}
pub struct PatchSummary {
    pub addr: Option<u32>,   // 1-based DMX addr, None for non-DMX
    pub universe: usize,
    pub mirror: bool,
    pub channel_count: usize,
    pub options: Vec<(String, String)>,  // patch-level key/value
}
```

`fixture_types` is populated once on startup from the `PATCHERS` distributed slice (static data).
`PatchOption` needs `#[derive(Clone)]` added in `src/fixture/patch/option.rs`.

### `src/console/command.rs`
Commands the GUI sends to Show:

```rust
pub enum ConsoleCommand {
    Repatch(Vec<FixtureGroupConfig>),
    AddOscClient(SocketAddr),
    RemoveOscClient(SocketAddr),
    RescanMidi,
}
```

### `src/console/app.rs`
The `ConsoleApp` struct implementing `eframe::App`. Contains:
- `shared: Arc<Mutex<ConsoleState>>`
- `commands: Sender<ConsoleCommand>`
- `draft: DraftPatch` — ephemeral editor state
- `close_handler: CloseHandler` — reuse from `animation_visualizer.rs`

**Layout** (two-panel):
```
┌─── Left Panel (320px) ──────┬─── Central Panel ───────────────────┐
│ Fixture type [dropdown]      │ Current Patch                        │
│ Group key [text]             │   ▸ CosmicBurst / Front             │
│ Channel [✓]  Color organ [✓] │       addr 1-6  univ 0  6ch         │
│                              │   ▸ FlashBang                        │
│ Group Options (dynamic)      │       addr 20   univ 0  5ch         │
│   paired: [✓]               │                                      │
│   max_intensity: [___]       ├─── OSC Clients ─────────────────────┤
│                              │ 192.168.1.50:9000   [Remove]         │
│ Patches                      │ [ip:port____________] [Add]          │
│ + [Add Patch]                │                                      │
│ ┌ addr [__] univ [_] [✗mir] ├─── MIDI Devices ────────────────────┤
│ │  Patch options (dynamic)   │ LaunchControlXL (connected)          │
│ └ [Remove]                   │ [Rescan MIDI]                        │
│                              │                                      │
│ [Add Group]  [Remove Group]  │                                      │
│ [Apply Patch]                │                                      │
│                              │                                      │
│ ⚠ last_error shown in red   │                                      │
└──────────────────────────────┴──────────────────────────────────────┘
```

**Draft patch state** (editor-local, not sent until Apply):
```rust
struct DraftPatch {
    groups: Vec<DraftGroup>,
    selected_idx: Option<usize>,
}
struct DraftGroup {
    fixture_type: String,
    key_override: String,  // empty = use fixture_type name
    channel: bool,
    color_organ: bool,
    group_option_values: Vec<(String, String)>,  // (key, raw string)
    patch_blocks: Vec<DraftPatchBlock>,
}
struct DraftPatchBlock {
    addr_str: String,
    universe_str: String,
    mirror: bool,
    patch_option_values: Vec<(String, String)>,
}
```

At **Apply**, the draft is serialized to `Vec<FixtureGroupConfig>` by building a `serde_yaml::Mapping`
from key/value pairs and deserializing. Errors surface in `last_error`. On success, `DraftPatch`
is re-seeded from the updated `ConsoleState::groups`.

On **first open** and after each successful repatch, `DraftPatch` is initialized from `ConsoleState::groups`
so the editor reflects live state.

**Dynamic option rendering** (`PatchOption` → widget):
- `Int` → `ui.text_edit_singleline(value)`
- `Bool` → `ui.checkbox(&mut parsed_bool, key)`
- `Select(variants)` → `egui::ComboBox`
- `SocketAddr` / `Url` → `ui.text_edit_singleline(value)` (validated at Apply time)

---

## Modified Files

### `src/fixture/patch/option.rs`
- Add `#[derive(Clone)]` to `PatchOption`

### `src/show.rs`
- Add `console: Option<ConsoleHandle>` field to `Show`
- Update `Show::new` signature to accept `Option<ConsoleHandle>`
- Add `push_console_state(&self)` — builds `ConsoleState` snapshot, writes under mutex
- Add `handle_console_commands(&mut self)` — drains `try_recv` loop:
  - `Repatch` → `self.patch.repatch()`, rebuild `Channels`, clear DMX buffers
  - `AddOscClient` / `RemoveOscClient` → `self.controller`
  - `RescanMidi` → new `rescan_midi()` method
- Call both methods in the main `run()` loop

### `src/main.rs`
- Add `mod console;`
- Add `--console` flag to `RunArgs`
- In `run_show`: if `--console`, create shared state + channel, spawn `Show::run` in a thread, call `run_console` on main thread; else run as today

### `src/control.rs` (if needed)
- Expose `replace_midi` method or `pub(crate)` field for `RescanMidi` support

---

## Implementation Order

1. `src/fixture/patch/option.rs` — `#[derive(Clone)]` on `PatchOption`
2. `src/console/state.rs` — snapshot types
3. `src/console/command.rs` — `ConsoleCommand`
4. `src/console/mod.rs` — handles + stub `run_console`
5. `src/show.rs` — `console` field, `push_console_state`, `handle_console_commands`, updated `new`
6. `src/main.rs` — `--console` flag + thread-split wiring
7. `src/console/app.rs` — skeleton `ConsoleApp`; confirm window opens
8. Current Patch panel — read-only `CollapsingHeader` per group
9. OSC panel — list + Add/Remove
10. MIDI panel — port list + Rescan
11. Patch Editor (left panel) — fixture dropdown, dynamic option widgets, Apply button
12. Draft init from live state — seed editor from `ConsoleState::groups` on open and after repatch

---

## Constraints

- `repatch()` cannot increase universe count — display this to the user on error
- `fixture_types` (PATCHERS metadata) is static — populate once at startup
- MIDI rescan replaces `MidiController` — may require small refactor of `Controller`
- `Options` is `serde_yaml::Mapping` — assemble from `Vec<(String, String)>` at Apply time

---

## Verification

```bash
cargo build

cargo run -- run --patch patch/test.yaml --quickstart --console

# Check:
# 1. Console window opens alongside running show
# 2. Current Patch panel shows fixtures from test.yaml
# 3. Edit a group option, Apply → show hot-reloads (DMX output changes)
# 4. Add/Remove OSC client → list updates
# 5. MIDI Rescan → list updates
# 6. Patch that would exceed universe count → red error message
# 7. Remove all patches from a group and apply → group disappears
```
