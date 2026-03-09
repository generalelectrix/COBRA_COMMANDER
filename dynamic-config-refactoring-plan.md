# Refactoring Plan: Dynamic Show Configuration via MetaCommand

## Context

Cobra Commander currently does extensive interactive configuration in `main.rs::run_show()` **before** the show loop starts: clock source selection, MIDI device confirmation, DMX port assignment, animation visualizer setup, and OSC controller pre-registration. The `--quickstart` flag already bypasses all of this with sensible defaults, proving the pattern works.

The goal is to invert this: **start the show immediately with defaults, then allow dynamic reconfiguration at runtime**. All meta-control commands should flow through the same `mpsc` event loop that handles OSC and MIDI input, via a new typed `MetaCommand` enum on `ControlMessage`.

**Excluded:** Patch file parsing stays as a CLI argument, parsed before the show starts.

---

## Step 1: Add `MetaCommand` plumbing and store zmq `Context` on `Show`

**What:** Add an empty `MetaCommand` enum and wire it into the event loop. Also store the zmq `Context` on `Show` so it's available for runtime service creation. Zero behavioral change.

**Files:**
- `src/control.rs` — Add `MetaCommand` enum (empty, or single `NoOp` variant to avoid compiler warnings). Add `ControlMessage::Meta(MetaCommand)` variant.
- `src/show.rs` — Add a `zmq_ctx: zmq::Context` field to `Show`. Accept it as a parameter in `Show::new()`. Add `ControlMessage::Meta(cmd) => self.handle_meta_command(cmd)` match arm in `Show::control()`. Add `fn handle_meta_command(&mut self, cmd: MetaCommand) -> Result<()>` that matches on the empty enum.
- `src/main.rs` — Pass the existing `zmq_ctx` into `Show::new()` instead of letting it drop after setup.

**Design notes:**
- `MetaCommand` lives in `control.rs` alongside `ControlMessage` — it's a top-level control concept, not tied to OSC or MIDI.
- Derive `Debug, Clone` to match `ShowControlMessage` style.
- Anyone with a `Sender<ControlMessage>` can send meta-commands. This is the key property enabling console GUI, OSC, CLI, or any future source to use the same path.
- The zmq `Context` is stored on `Show` from the start because multiple later steps need it (animation visualizer in Step 4, clock service in Step 7). `zmq::Context` is cheaply cloneable (it's reference-counted internally), so passing it around is lightweight.

**Complexity:** Trivial. Pure boilerplate.

---

## Step 2: Migrate existing OSC "Meta" handlers to typed `MetaCommand`

**What:** Convert the three string-dispatched OSC meta handlers into proper `MetaCommand` variants.

**Currently:** `show.rs` lines 225-263 match on `msg.group() == "Meta"` then string-match on `msg.control()` for `"ReloadPatch"`, `"RefreshUI"`, `"ResetAllAnimations"`. The handler logic is inlined.

**After:**
- Add three variants to `MetaCommand`: `ReloadPatch`, `RefreshUI`, `ResetAllAnimations`
- Move the handler bodies into `handle_meta_command()`
- The OSC `"Meta"` match arm now converts the OSC message into the appropriate `MetaCommand` variant and delegates

**Files:**
- `src/control.rs` — Add three variants to `MetaCommand`
- `src/show.rs` — Extract handler bodies from `handle_osc_message` into `handle_meta_command`. The OSC "Meta" arm becomes a thin translation layer.

**Default:** N/A (no startup behavior change)

**Complexity:** Low. Mechanical code movement.

---

## Step 3: Make OSC pre-registration dynamic

**What:** Remove the `prompt_osc_config()` call. Start with no pre-registered controllers.

**Currently:** `main.rs` lines 179-183 call `prompt_osc_config()` to pre-register known OSC controllers.

**After:** Start with empty `osc_controllers` vec. The existing auto-registration in `OscListener::run()` already handles dynamic client registration — clients are auto-registered on first message. This step is essentially just deleting the prompt.

**Files:**
- `src/main.rs` — Remove the `prompt_osc_config` call and conditional. Always pass `vec![]`.

**Default for immediate start:** No pre-registered controllers. Auto-registration handles everything (already matches quickstart behavior).

**Complexity:** Trivial. Smallest possible migration — the dynamic path already exists.

---

## Step 4: Make animation visualizer dynamic

**What:** Remove the animation visualizer prompt. Allow starting/stopping it at runtime.

**Currently:** `main.rs` lines 168-172 prompt whether to run the visualizer, and lines 246-248 launch the subprocess. `Show.animation_service` is already `Option<AnimationPublisher>`.

**After:**
- Add `MetaCommand::StartAnimationVisualizer` and `MetaCommand::StopAnimationVisualizer`
- `handle_meta_command` creates/destroys the `AnimationPublisher` and launches/kills the subprocess
- Start with `animation_service: None`

**Files:**
- `src/control.rs` — Add two `MetaCommand` variants
- `src/show.rs` — Handle start/stop in `handle_meta_command`. The `Option<AnimationPublisher>` field already supports this.
- `src/main.rs` — Remove the prompt. Pass `None` for `animation_service`. Remove the `launch_animation_visualizer()` call.

**Design note:** `Show` already holds the zmq `Context` (added in Step 1), so creating a new `AnimationPublisher` at runtime is straightforward — just call `animation_publisher(&self.zmq_ctx)`.

**Default for immediate start:** No animation visualizer. Matches quickstart.

**Complexity:** Low. The `Option` pattern is already in place, and the zmq context is available on `Show`.

---

## Step 5: Make DMX port assignment dynamic

**What:** Start with offline ports for all universes. Allow reassigning ports at runtime.

**Currently:** `main.rs` lines 223-244 list available ports and prompt the user to assign each universe.

**After:**
- Add `MetaCommand` variants for port management (e.g., `ListDmxPorts`, `AssignDmxPort { universe, port_id }`)
- `Show` gets a method to swap a DMX port for a given universe
- Start with `OfflineDmxPort` for all universes

**Files:**
- `src/control.rs` — Add DMX port management variants to `MetaCommand`. Use a port identifier (not the trait object itself) to keep `MetaCommand` cloneable. Something like `DmxPortId` enum: `Offline`, `Serial(usize)`, `Artnet(String)`.
- `src/show.rs` — `handle_meta_command` swaps ports. Port discovery (`available_ports()`) can be called on demand.
- `src/main.rs` — Remove DMX port prompting. Always start with offline ports.

**Design note:** Port discovery involves I/O (serial port enumeration, artnet polling). Should this happen on the event loop thread, or be delegated to a background thread that sends results back via the channel? For now, synchronous on the event loop is fine — port discovery is fast except for artnet polling, which can be made opt-in.

**Default for immediate start:** All universes get `OfflineDmxPort`. Show runs but produces no DMX output until ports are assigned. Safe and deterministic.

**Complexity:** Moderate. The `DmxPortId` abstraction and port swapping logic are new, but straightforward.

---

## Step 6: Make MIDI configuration dynamic

**What:** Start with auto-discovered devices (no confirmation prompt). Allow adding/removing devices at runtime.

**Currently:** `main.rs` lines 185-213 auto-discover devices, prompt for confirmation, optionally do manual config.

**After:**
- Add `MetaCommand` variants: `RescanMidi`, `AddMidiDevice(DeviceSpec)`, `RemoveMidiDevice(DeviceId)`
- `MidiController` needs `add_device()` / `remove_device()` methods
- Start with auto-discovered devices, no confirmation prompt

**Files:**
- `src/control.rs` — Add MIDI management variants to `MetaCommand`
- `src/midi/mod.rs` — Add `add_device()`, `remove_device()`, `rescan()` methods to `MidiController`
- `src/show.rs` — `handle_meta_command` delegates MIDI commands to `self.controller`
- `src/control.rs` (Controller struct) — Expose MIDI management methods forwarding to `self.midi`
- `src/main.rs` — Remove the confirmation prompt. Keep auto-discovery, skip the `prompt_bool("Does this look correct?")` and manual `prompt_midi()` path.

**Design note:** The `midi_harness` crate's `DeviceManager` may already support dynamic add/remove. If so, this is simpler. If not, `MidiController` needs to track connections and manage teardown.

**Default for immediate start:** Auto-discovered devices connected without prompting. The auto-discovery logic (`Device::auto_configure`) is already good.

**Complexity:** Moderate. Depends on `midi_harness` API capabilities.

---

## Step 7: Make clock source dynamic

**What:** Start with internal clocks (no audio). Allow switching clock source at runtime.

**Currently:** `main.rs` lines 154-164 prompt for remote clock service vs internal clocks, and if internal, which audio device.

**After:**
- Add `MetaCommand::SetClockSource(ClockSourceConfig)` where `ClockSourceConfig` captures the choice (internal with optional audio device name, or service endpoint)
- `Clocks` needs a method to switch between variants at runtime

**Files:**
- `src/control.rs` — Add `MetaCommand::SetClockSource` variant with a `ClockSourceConfig` enum
- `src/clocks.rs` — Add a `reconfigure(&mut self, config: ClockSourceConfig)` method. This must handle tearing down the current clock source (especially `Clocks::Service` which has a zmq subscriber thread) and creating a new one. The zmq `Context` needs to be available.
- `src/show.rs` — `handle_meta_command` delegates to `self.clocks.reconfigure()`
- `src/main.rs` — Remove clock prompting. Start with `Clocks::internal(None)`.

**Design note:** This is the most complex migration. `Clocks::Service` wraps a `ClockService` which holds zmq sockets and a background subscriber. Switching away from it requires clean teardown. Switching to it requires the zmq context, which is already stored on `Show` (added in Step 1). `ClockService` should handle its own lifecycle via `Drop` for clean teardown. Audio device initialization (`AudioInput::new`) can also fail, so error handling for dynamic switching matters.

**Default for immediate start:** `Clocks::internal(None)` — internal clocks, no audio input. Matches quickstart.

**Complexity:** High. Thread lifecycle management, zmq context threading, error recovery on failed switches.

---

## Recommended Implementation Order

```
Step 1 (plumbing)
  |
  +---> Step 2 (migrate existing Meta handlers) -- low, proves the pattern
  +---> Step 3 (OSC pre-registration) ----------- trivial, just delete a prompt
  +---> Step 4 (animation visualizer) ------------ low-moderate
  +---> Step 5 (DMX ports) ----------------------- moderate
  +---> Step 6 (MIDI devices) -------------------- moderate
  +---> Step 7 (clock source) -------------------- high complexity
```

Steps 2-7 are all independent of each other. Only Step 1 is a prerequisite. Order within 2-7 is by increasing complexity so early steps build confidence in the pattern before tackling harder migrations.

---

## Static Configuration Inventory

For completeness, every piece of pre-show configuration in `run_show()`:

| Config Area | Current Location | Quickstart Default | Dynamic Migration Step |
|---|---|---|---|
| Patch file | `main.rs:150` | N/A (always required) | **Excluded** (stays as CLI arg) |
| Clock source | `main.rs:154-164` | `Clocks::internal(None)` | Step 7 |
| Animation visualizer | `main.rs:168-172` | `None` | Step 4 |
| OSC pre-registration | `main.rs:179-183` | `vec![]` | Step 3 |
| MIDI devices | `main.rs:185-213` | Auto-discover, no prompt | Step 6 |
| DMX ports | `main.rs:223-244` | Auto-assign available, fill with Offline | Step 5 |

---

## Verification

- After Step 1: `cargo check` passes, no behavioral change
- After Step 2: OSC Meta commands still work identically, now routed through `handle_meta_command`
- After each migration step: `--quickstart` behavior is unchanged; non-quickstart runs start immediately with defaults equivalent to quickstart; the removed prompts' functionality is available via `MetaCommand`
- End state: `run_show()` does patch parsing, auto-discovers MIDI, creates controller + show, and calls `show.run()`. All other configuration happens dynamically.
