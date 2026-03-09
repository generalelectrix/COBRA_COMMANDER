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

## Step 6: Make MIDI configuration dynamic

**What:** Start with auto-discovered devices (no confirmation prompt). Offer reconfiguration in the CLI config thread.

**main.rs changes:**
- Remove the confirmation prompt (`prompt_bool("Does this look correct?")`) and manual `prompt_midi()` path
- Keep auto-discovery: `Device::auto_configure(internal_clocks, &midi_inputs, &midi_outputs)` stays
- Delete the disabled color organ block (`if false { ... }`)

**control.rs changes:**
- Add `MetaCommand::ReconfigureMidi(Vec<DeviceSpec<Device>>)` — replaces the entire MIDI device set

**Controller / MidiController changes:**
- `Controller` needs a `reconfigure_midi(&mut self, devices: Vec<DeviceSpec<Device>>) -> Result<()>` method that tears down existing connections and sets up new ones
- This requires `MidiController` to support teardown + rebuild. Check if `midi_harness` supports this; if not, drop and recreate `MidiController` entirely.

**show.rs changes:**
- `handle_meta_command` matches `ReconfigureMidi`, calls `self.controller.reconfigure_midi(devices)`, then `self.refresh_ui()`

**cli.rs changes:**
- Add `prompt_reconfigure_midi(client: &CommandClient) -> Result<Option<CommandResponse>>` action function
- Lists ports, runs `prompt_midi()`, sends `ReconfigureMidi` command
- Wire into `run_cli_configuration` via `offer_action`

**Dependency on internal_clocks:** `Device::auto_configure` and `Device::all` take `internal_clocks: bool`. Pass `internal_clocks` into the CLI thread at spawn time (captures the initial value). If Step 7 adds dynamic clock switching later, it should update the MIDI devices too.

**Files:** `src/control.rs`, `src/show.rs`, `src/cli.rs`, `src/main.rs`, `src/control.rs` (Controller), possibly `src/midi/mod.rs` (MidiController)

**Complexity:** Moderate. Depends on `midi_harness` API capabilities.

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
pub(crate) fn run_cli_configuration(client: CommandClient) -> Result<()> {
    offer_action(&client, prompt_set_clock_source)?;
    offer_action(&client, prompt_reconfigure_midi)?;
    offer_action(&client, prompt_assign_dmx_ports)?;
    offer_action(&client, prompt_start_animation_visualizer)?;
    Ok(())
}
```

Clock source first (affects MIDI), then MIDI, then DMX (independent), then animation visualizer.

---

## Implementation Order

```
Steps 1-4.5: DONE
  |
  +---> Step 5 (DMX ports) ---- moderate, pure show-side, no external deps
  +---> Step 6 (MIDI devices) - moderate, depends on MidiController teardown
  +---> Step 7 (clock source) - high, thread lifecycle, cross-cutting with MIDI
```

Steps 5-6 are independent. Step 7 should come last because clock mode changes affect MIDI configuration (Step 6).

---

## Verification

After each step:
1. `cargo check -j 2` — compiles cleanly
2. `cargo clippy -j 2` — no new warnings
3. `--quickstart` behavior unchanged (no CLI thread, auto defaults)
4. Non-quickstart: show starts immediately, CLI thread offers configuration against the running show
5. Each prompt sends a `MetaCommand` via `CommandClient`, show processes it and replies
6. Errors in commands are displayed and user is offered retry via `offer_action`

End state: `run_show()` does only: parse patch, auto-discover MIDI, create controller + show with all-default config, spawn CLI thread, `show.run()`. All interactive configuration happens in the CLI thread against the running show.
