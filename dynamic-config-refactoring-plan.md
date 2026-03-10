# Refactoring Plan: Dynamic Show Configuration via MetaCommand

## Context

Cobra Commander currently does extensive interactive configuration in `main.rs::run_show()` **before** the show loop starts: clock source selection, MIDI device confirmation, DMX port assignment, animation visualizer setup, and OSC controller pre-registration. The `--quickstart` flag already bypasses all of this with sensible defaults, proving the pattern works.

The goal is to invert this: **start the show immediately with defaults, then allow dynamic reconfiguration at runtime**. All meta-control commands should flow through the same `mpsc` event loop that handles OSC and MIDI input, via a new typed `MetaCommand` enum on `ControlMessage`.

**Excluded:** Patch file parsing stays as a CLI argument, parsed before the show starts.

---

## Step 1: Add `MetaCommand` plumbing and store zmq `Context` on `Show` ✅ DONE

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

---

## Step 2: Migrate existing OSC "Meta" handlers to typed `MetaCommand` ✅ DONE

**What:** Convert the three string-dispatched OSC meta handlers into proper `MetaCommand` variants.

**After:**
- Added three variants to `MetaCommand`: `ReloadPatch`, `RefreshUI`, `ResetAllAnimations`
- Moved handler bodies into `handle_meta_command()`
- OSC `"Meta"` match arm now converts via `meta_command_from_osc()` and delegates

---

## Step 3: Make OSC pre-registration dynamic ✅ DONE

**What:** Removed `prompt_osc_config()` call. Start with no pre-registered controllers. Auto-registration in `OscListener::run()` handles dynamic client registration.

---

## Step 4: Make animation visualizer dynamic ✅ DONE

**What:** Removed the animation visualizer prompt. Added `MetaCommand::StartAnimationVisualizer`. `handle_meta_command` creates the `AnimationPublisher` and launches the subprocess on demand. Start with `animation_service: None`.

---

## Step 4.5: Command-Response Notification System ✅ DONE

**What:** Added a reply mechanism so command senders can learn whether a command succeeded or failed. This enables moving interactive prompts off the startup path into a CLI configuration thread.

**Infrastructure built:**
- `CommandResponse` — type alias for `Result<(), String>` (`src/control.rs`)
- `CommandClient` — cloneable handle that sends `MetaCommand`s and blocks for responses (`src/control.rs`)
- `ControlMessage::Meta(MetaCommand, Option<Sender<CommandResponse>>)` — single variant with optional reply channel
- `offer_action()` — retry-on-error wrapper for CLI prompt functions (`src/cli.rs`)
- `run_cli_configuration()` — runs in a thread alongside `show.run()`, sequentially offers configuration actions (`src/cli.rs`)
- CLI thread spawned in `main.rs` unless `--quickstart`

**Pattern:** Each configuration action is a function `fn(&CommandClient) -> Result<Option<CommandResponse>>` that prompts the user, builds a `MetaCommand`, sends it via `CommandClient::send_command()`, and returns the response. `offer_action()` handles retry-on-error.

---

## Step 5: Make DMX port assignment dynamic

**What:** Start with offline ports for all universes (non-quickstart). Offer port assignment in the CLI config thread.

**main.rs changes:**
- Remove the non-quickstart DMX port prompt loop (lines 148-153)
- Non-quickstart starts with offline ports: `vec![Box::new(OfflineDmxPort) as Box<dyn DmxPort>; universe_count]`
- Keep quickstart's existing auto-assign behavior (lines 141-147) — quickstart needs DMX output immediately and doesn't spawn the CLI thread

**control.rs changes:**
- Add `MetaCommand::AssignDmxPort { universe: usize, port: Box<dyn DmxPort> }`
- Pass the trait object directly — `DmxPort` is `Send` (recently fixed in `rust_dmx` 0.7), so it crosses the channel safely
- Drop `Clone` from `MetaCommand` — nothing clones it, and `ControlMessage` doesn't derive `Clone` either
- For `Debug`: custom `Debug` impl for the variant since `Box<dyn DmxPort>` isn't `Debug` but is `Display`
- No intermediate `DmxPortSpec` abstraction needed

**show.rs changes:**
- `handle_meta_command` matches `AssignDmxPort { universe, mut port }`:
  - Validates `universe < self.dmx_ports.len()`
  - Calls `port.open()` — if it fails, includes the open error in the response
  - Swaps the opened port into `self.dmx_ports[universe]`
  - Zeros the DMX buffer for the reassigned universe

**cli.rs changes:**
- Add `prompt_assign_dmx_ports(client: &CommandClient) -> Result<Option<CommandResponse>>` action function
- Calls `available_ports()` to discover ports, prompts the user to select a port for each universe
- Sends one `AssignDmxPort` per universe with the unopened `Box<dyn DmxPort>`
- Wire into `run_cli_configuration` via `offer_action`

**Design note:** Port discovery and user prompting happen in the CLI thread. The unopened `Box<dyn DmxPort>` is sent through the channel. The show calls `open()` and reports success/failure via the response. Edge cases around port remapping (moving ports between universes) are deferred.

**Files:** `src/control.rs`, `src/show.rs`, `src/cli.rs`, `src/main.rs`

**Complexity:** Low-moderate. No new abstraction layer — just pass the trait object through.

---

## Step 6: Add MIDI devices dynamically

**What:** Add individual MIDI devices at runtime via `MetaCommand::AddMidiDevice`. Non-quickstart starts with zero MIDI devices — the CLI config thread runs the current auto-discover → confirm → reconfigure workflow, then sends each device one-by-one to the show. The same MetaCommand will serve a future GUI device picker.

**Why incremental add instead of bulk replace:**
- `DeviceManager` already supports `add_from_spec()` for individual devices
- Adding to the existing manager preserves all slot state, including reconnect tracking
- A GUI wants "add this device" not "replace everything with this new list"
- Simpler error model: one device fails, others unaffected

**main.rs changes:**
- Remove the entire MIDI configuration block: auto-discovery, confirmation prompt, `prompt_midi` fallback, and the disabled color organ block (lines 99-121)
- Non-quickstart now starts with zero MIDI devices — the CLI config thread handles all MIDI setup
- Keep quickstart's existing behavior: `Device::auto_configure` runs at startup and devices are passed directly to the controller (no CLI thread, no prompts)
- Move the color organ block (currently `if false { ... }`) into the CLI config thread action (see cli.rs below) — it stays disabled but lives in the right place for future enablement

**control.rs changes:**
- Add `MetaCommand::AddMidiDevice(DeviceSpec<Device>)` — adds a single device
- `DeviceSpec` is already `Clone + Debug`, and `MetaCommand` already dropped `Clone` in Step 5

**midi/mod.rs changes (MidiController):**
- Add `pub fn add_device(&mut self, spec: DeviceSpec<Device>) -> Result<()>` to `MidiController`
- Delegates to `self.0.borrow_mut().add_from_spec(spec.device, spec.input_id, spec.output_id)`
- This creates a new slot with input+output connections. The slot immediately participates in the reconnect system — if the device disconnects and reappears, `try_reconnect` will reconnect it automatically.

**show.rs changes (Controller):**
- Add `pub fn add_midi_device(&mut self, spec: DeviceSpec<Device>) -> Result<()>` to `Controller`
- Delegates to `self.midi.add_device(spec)`
- `handle_meta_command` matches `AddMidiDevice(spec)`:
  - Calls `self.controller.add_midi_device(spec)?`
  - Calls `self.refresh_ui()` to update any connected displays
  - Returns success/error via reply channel

**cli.rs changes:**
- Add `prompt_configure_midi(client: &CommandClient, internal_clocks: bool) -> Result<Option<CommandResponse>>` action function
- This mirrors the current main.rs MIDI workflow, moved to the CLI config thread:
  1. Call `midi_harness::list_ports()` to get current inputs/outputs
  2. Run `Device::auto_configure(internal_clocks, &inputs, &outputs)` to find matching devices
  3. Display discovered devices (same format as current main.rs output)
  4. Ask `prompt_bool("Does this look correct?")`
  5. If yes → use the auto-discovered list as-is
  6. If no → call `prompt_midi(&inputs, &outputs, Device::all(internal_clocks))` for manual reconfiguration
  7. Send each resulting `DeviceSpec` one-by-one as `MetaCommand::AddMidiDevice(spec)` via `CommandClient`, reporting success/failure for each
- Include the color organ block (still gated behind `if false { ... }` or a prompt) — moved here from main.rs
- Wire into `run_cli_configuration` via `offer_action`

**Dependency on internal_clocks:** `Device::auto_configure` and `Device::all` take `internal_clocks: bool`. Pass `internal_clocks` into the CLI thread at spawn time (captures the initial value). If Step 7 adds dynamic clock switching later, it should update the MIDI devices too.

**Auto-reconnect preservation:** Adding a device via `add_from_spec` creates a `DeviceSlot` with the device's `DeviceId` for both input and output. The existing `handle_device_change` → `try_reconnect` path checks ALL slots (including newly added ones). No changes needed to the reconnect system.

**midi_harness changes needed:** None — `add_from_spec()`, `add_slot()`, `connect_input()`, `connect_output()` already exist.

**Files:** `src/control.rs`, `src/show.rs`, `src/cli.rs`, `src/main.rs`, `src/midi/mod.rs`

**Complexity:** Low. All building blocks exist.

---

## Step 6.5: Empty MIDI device slots

**What:** Clear the device assignment from a slot, leaving it empty. The slot itself remains (slots will be patch-driven in future work). No CLI config thread action — this API exists for future GUI use.

**Slot state model:**

Slots have three states:

| State | `input`/`output` fields | Reconnect behavior |
|-------|------------------------|--------------------|
| **Empty** | `None` / `None` | No — nothing to reconnect |
| **Populated + Disconnected** | `Some` with `port: None` | Yes — `try_reconnect` re-establishes when device reappears |
| **Populated + Connected** | `Some` with `port: Some(...)` | N/A — already connected |

- `add_from_spec` transitions Empty → Populated + Connected
- Physical device disappearing transitions Populated + Connected → Populated + Disconnected
- Physical device reappearing transitions Populated + Disconnected → Populated + Connected
- **Step 6.5's "empty" operation** transitions Populated (either sub-state) → Empty

**midi_harness changes (new API):**
- Add `pub fn clear_slot(&mut self, name: &str) -> Result<()>` to `DeviceManager`
- Sets `slot.input = None` and `slot.output = None`, dropping any active connections
- The slot remains in `self.slots` with its `name` and `model` intact — just no device assigned
- Returns error if no slot with that name exists
- After clearing, `try_reconnect` won't match this slot (no `DeviceId` to match against)

**control.rs changes:**
- Add `MetaCommand::ClearMidiDevice(String)` — empties the named slot

**midi/mod.rs changes (MidiController):**
- Add `pub fn clear_device(&mut self, name: &str) -> Result<()>` — delegates to `clear_slot`
- Add `pub fn device_names(&self) -> Vec<String>` — for future GUI use

**show.rs changes (Controller):**
- Add `pub fn clear_midi_device(&mut self, name: &str) -> Result<()>`
- `handle_meta_command` matches `ClearMidiDevice(name)`, calls clear, then `self.refresh_ui()`

**cli.rs changes:** None — no CLI config thread action for clearing.

**Auto-reconnect interaction:** After clearing, the slot has no `DeviceId` entries. `try_reconnect` won't match it. The slot is empty until re-populated via `AddMidiDevice`.

**Files:** `tunnels/midi_harness/src/lib.rs`, `src/control.rs`, `src/show.rs`, `src/midi/mod.rs`

**Complexity:** Low-moderate.

---

## Step 7: Make clock source dynamic

**What:** Start with internal clocks (no audio). Offer clock source switching in the CLI config thread.

**main.rs changes:**
- Remove clock prompting (lines 74-84). Always start with `Clocks::internal(None)`.
- `internal_clocks` becomes `true` unconditionally at startup
- Remove `use clock_service::prompt_start_clock_service`, `use tunnels::audio::prompt_audio`

**control.rs changes:**
- Add `MetaCommand::SetClockSource(ClockSourceConfig)` where:
  ```rust
  enum ClockSourceConfig {
      Internal { audio_device: Option<String> },
      Service { /* provider info from clock_service */ },
  }
  ```
- Must be `Clone + Debug` for `MetaCommand` derives

**show.rs changes:**
- `handle_meta_command` matches `SetClockSource`, replaces `self.clocks` with the new variant
- For `ClockSourceConfig::Service`: needs `self.zmq_ctx` (already stored on `Show`) to create a `ClockService`
- For `ClockSourceConfig::Internal`: creates `AudioInput` from the device name
- On clock mode change: `internal_clocks` changes, which may invalidate MIDI auto-configuration

**clocks.rs / clock_service.rs changes:**
- No `reconfigure` method needed on `Clocks` — `Show` replaces `self.clocks` entirely
- `ClockService` holds an `Arc<Mutex<SharedClockData>>` and a spawned thread that loops on blocking zmq recv. When dropped, the thread may leak. Options: accept the leak (thread errors when zmq context cleans up), add a `Drop` impl that shuts down the zmq socket, or use non-blocking recv with a poison flag.

**cli.rs changes:**
- Add `prompt_set_clock_source(client: &CommandClient) -> Result<Option<CommandResponse>>` action function
- Reuses the existing prompt logic from `clock_service::prompt_start_clock_service` and `tunnels::audio::prompt_audio`
- Wire into `run_cli_configuration` via `offer_action`

**Files:** `src/control.rs`, `src/show.rs`, `src/cli.rs`, `src/main.rs`, `src/clocks.rs`, `src/clock_service.rs`

**Complexity:** High. Thread lifecycle management, zmq context threading, error recovery on failed switches.

---

## CLI Thread Configuration Sequence

After all steps, `run_cli_configuration` offers actions in this order:

```rust
pub(crate) fn run_cli_configuration(client: CommandClient, internal_clocks: bool) -> Result<()> {
    offer_action(&client, prompt_set_clock_source)?;
    offer_action(&client, |c| prompt_configure_midi(c, internal_clocks))?;
    offer_action(&client, prompt_assign_dmx_ports)?;
    offer_action(&client, prompt_start_animation_visualizer)?;
    Ok(())
}
```

Clock source first (affects MIDI), then MIDI (auto-discover → confirm → add one-by-one), then DMX (independent), then animation visualizer.

---

## Implementation Order

```
Steps 1-4.5: DONE
  |
  +---> Step 5 (DMX ports) ---- moderate, pure show-side, no external deps
  +---> Step 6 (MIDI add) ----- low, all building blocks exist in midi_harness
  +---> Step 6.5 (MIDI clear) - low-moderate, new clear_slot in midi_harness
  +---> Step 7 (clock source) - high, thread lifecycle, cross-cutting with MIDI
```

Steps 5 and 6 are independent. Step 6.5 follows Step 6. Step 7 should come last because clock mode changes affect MIDI configuration (Step 6).

---

## Verification

After each step:
1. `cargo check -j 2` — compiles cleanly
2. `cargo clippy -j 2` — no new warnings
3. `--quickstart` behavior unchanged (no CLI thread, auto defaults)
4. Non-quickstart: show starts immediately, CLI thread offers configuration against the running show
5. Each prompt sends a `MetaCommand` via `CommandClient`, show processes it and replies
6. Errors in commands are displayed and user is offered retry via `offer_action`

End state: `run_show()` does only: parse patch, create controller + show with all-default config, spawn CLI thread, `show.run()`. All interactive configuration (including MIDI discovery) happens in the CLI thread against the running show.
