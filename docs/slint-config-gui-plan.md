# Plan: Slint Config GUI for Cobra Commander

## Context

Cobra Commander currently uses an interactive CLI thread for show configuration (clocks, MIDI, DMX ports). This is sequential, text-based, and one-shot — once you've gone through the prompts, you can't reconfigure without restarting. We want a persistent Slint GUI window that replaces this with a graphical interface, allowing reconfiguration at any time during the show.

## Scope

Convert these CLI config actions into GUI equivalents:
1. **Clock configuration** — internal clocks (with optional audio device) vs external ZMQ clock service
2. **MIDI device configuration** — scan, auto-configure, add/clear devices
3. **DMX port assignment** — scan ports, assign to universes
4. **Launch animation visualizer** — button to spawn viz subprocess

Leave the animation visualizer (`visualizer.rs`, `ui/visualizer.slint`) untouched.

## Architecture

The GUI sends `MetaCommand` messages through the existing `CommandClient`, identical to how the CLI thread works. The GUI runs on the main thread (Slint/winit requirement), so the show loop moves to a background thread when `--gui` is active.

```
Main thread: Slint event loop (ConfigPanel window)
     │
     │  CommandClient.send_command(MetaCommand::*)
     ▼
Background thread: Show.run() loop (25.3ms interval)
```

## Files to Modify/Create

### 1. `src/cli.rs` — Add `--gui` flag

Add to `RunArgs`:
```rust
/// Run the Slint configuration GUI instead of the interactive CLI.
#[arg(long)]
pub gui: bool,
```

### 2. `build.rs` — Compile new .slint file

```rust
fn main() {
    slint_build::compile("ui/visualizer.slint").unwrap();
    slint_build::compile("ui/config_panel.slint").unwrap();
}
```

### 3. `ui/config_panel.slint` (NEW) — GUI layout

Four sections, each with scan/configure controls:

- **Clocks section**: Toggle between internal/external. Clock providers appear live via persistent DNS-SD browse (no "Browse" button needed — they just show up). Audio device selector for internal mode.
- **MIDI section**: "Scan" button discovers devices. Shows auto-configured devices. Add/remove buttons.
- **DMX section**: "Scan" button finds ports. Per-universe dropdown to assign ports. Shows universe count from patch.
- **Visualizer section**: "Launch" button.
- **Status bar**: Feedback messages.

Use Slint's `std-widgets.slint` components (`Button`, `ComboBox`, `ListView`, `GroupBox`, `StandardButton`).

Callbacks and properties declared for each action — the Rust side wires these to `CommandClient`.

### 4. `src/gui.rs` (NEW, ~200-250 lines) — GUI ↔ Show bridge

Entry point: `pub fn run_gui(client: CommandClient, universe_count: usize) -> Result<()>`

Key patterns:
- `slint::include_modules!()` generates `ConfigPanel` from the .slint file
- Each Slint callback captures a `CommandClient` clone and calls `send_command`
- Scan operations (MIDI ports, DMX ports) run on `std::thread::spawn`, push results back via `slint::invoke_from_event_loop`
- Clock providers: `browse_forever` called directly with closures that invoke `slint::invoke_from_event_loop` — providers appear/disappear in the UI in real time, zero polling
- Discovered hardware stored in `Arc<Mutex<Vec<...>>>` so callbacks can index into the real objects when user selects by position

Callback wiring sketch:

| GUI Action | Background Work | MetaCommand Sent |
|---|---|---|
| Clock providers (live) | `browse_forever` closures → `invoke_from_event_loop` | — (updates dropdown) |
| Select clock provider | `subscriber.subscribe(provider)` on background thread | `UseClockService(ClockService)` |
| Use internal clocks | None | — (default state) |
| Select audio device | None | `SetAudioDevice(name)` |
| Scan MIDI | `list_ports()` + `Device::auto_configure()` | — (populates list) |
| Add MIDI device | None | `AddMidiDevice(spec)` |
| Clear MIDI device | None | `ClearMidiDevice { slot_name }` |
| Scan DMX ports | `available_ports(artnet_timeout)` | — (populates dropdowns) |
| Assign DMX port | None | `AssignDmxPort { universe, port }` |
| Launch visualizer | None | `StartAnimationVisualizer` |
| Reload patch | None | `ReloadPatch` |

### 5. `src/clock_service.rs` — Extract non-interactive connect helper

Add one function for GUI use (existing `prompt_start_clock_service` stays for CLI mode):

```rust
/// Connect to a resolved clock provider and return the ClockService.
/// Used by the GUI after browse_forever resolves a service.
pub fn connect_to_clock_provider(ctx: &Context, host: &str, port: u16) -> Result<ClockService> {
    // Create ZMQ SUB socket, connect, spawn receiver thread
    // (extracted from the guts of prompt_start_clock_service)
}
```

No changes to the tunnels crate.

### 5b. GUI clock browsing — call `browse_forever` directly

`browse_forever` (`tunnels/zero_configure/src/bare.rs:134`) is public and takes two closures:
- `on_service_appear: FnMut((ResolveResult, String))`
- `on_service_drop: FnMut(&str)`

The GUI calls it directly on a spawned thread with closures that:
1. Call `slint::invoke_from_event_loop` with the resolved info (name, host, port) to update the Slint model directly on the GUI thread

```rust
// In gui.rs — no tunnels crate changes needed
let weak = window.as_weak();

thread::spawn(move || {
    browse_forever(
        CLOCK_SERVICE_NAME,
        {
            let weak = weak.clone();
            move |(resolved, name)| {
                let host = resolved.host_target.clone();
                let port = resolved.port;
                let weak = weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(win) = weak.upgrade() {
                        // Add provider (name, host, port) to Slint model
                    }
                }).ok();
            }
        },
        {
            let weak = weak.clone();
            move |name| {
                let name = name.to_string();
                let weak = weak.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(win) = weak.upgrade() {
                        // Remove provider from Slint model
                    }
                }).ok();
            }
        },
    );
});
```

All state lives on the GUI thread. The Slint model holds provider structs with `{name, host, port}`. When the user selects a provider, the callback reads host/port from the model item and calls `connect_to_clock_provider`. No shared state, no mutexes. Fully event-driven, zero polling, zero tunnels coupling.

### 6. `src/main.rs` — Thread restructuring for GUI mode

Add `mod gui;` declaration. Modify `run_show()`:

```rust
if args.gui {
    // Show runs on background thread; GUI takes main thread (Slint requirement)
    std::thread::spawn(move || show.run());
    gui::run_gui(command_client, universe_count)?;
} else if !args.quickstart {
    // Existing CLI config thread
    let cli_client = command_client.clone();
    std::thread::spawn(move || { ... });
    show.run();
} else {
    show.run();
}
```

### 7. `slint::include_modules!()` placement

Currently called in `visualizer.rs:15`. Since both `visualizer.rs` and `gui.rs` need generated types, and the visualizer runs as a separate subprocess (`cobra_commander viz`), the two never coexist in the same process. Both files can call `include_modules!()` independently — the compiler generates all types in both, but only one window is used per invocation.

## Implementation Order

1. `cli.rs` — add `--gui` flag (trivial)
2. `build.rs` — add second compile call
3. `ui/config_panel.slint` — design the GUI layout
4. `clock_service.rs` — extract `connect_to_clock_provider` helper
5. `gui.rs` — implement the bridge module (including direct `browse_forever` call)
6. `main.rs` — add `mod gui` and thread restructuring

One tiny tunnels change: make `SERVICE_NAME` in `tunnels/tunnels/src/clock_server.rs:17` public so `gui.rs` can pass it to `browse_forever`.

## Verification

1. `cargo check` — everything compiles
2. `cargo run -- run patch/test.yaml --gui` — GUI window opens, show runs in background
3. Test each config action:
   - Click "Scan MIDI" → devices appear in list
   - Add a MIDI device → status confirms it was added
   - Scan DMX ports → ports appear in dropdowns
   - Assign a port to a universe → status confirms
   - Browse clock providers → providers appear (if a tunnels controller is running)
   - Launch visualizer → viz window opens
4. Close GUI → process exits cleanly
5. `cargo run -- run patch/test.yaml` (no `--gui`) — existing CLI flow still works unchanged
