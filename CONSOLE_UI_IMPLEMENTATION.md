# Console UI — Refined Implementation Breakdown

Companion to `CONSOLE_UI_PLAN.md`. Adds codebase-validated analysis, tool integration, and concrete work packages.

---

## Codebase Findings (Plan Validation)

After deep reading of `show.rs`, `control.rs`, `config.rs`, `fixture/patch/mod.rs`, `fixture/patch/option.rs`, `fixture/patch/patcher.rs`, `animation_visualizer.rs`, and `main.rs`:

### Confirmed Correct in Plan
- **Threading model**: `AnimationVisualizer` uses `Arc<Mutex<>>` + separate thread — plan correctly follows this pattern
- **`Patch::repatch()`** already accepts `&[FixtureGroupConfig]` in-memory (not just files) — `ConsoleCommand::Repatch` will work directly
- **`PatchOption` needs `#[derive(Clone)]`** — confirmed missing at `option.rs:91`
- **`PATCHERS` distributed slice** is static and available for `FixtureTypeMeta` population
- **`CloseHandler`** exists in `animation_visualizer.rs` and can be reused
- **`repatch()` universe count check** is built in (`patch/mod.rs:126-131`)

### Gaps / Risks Identified

1. **Thread inversion required**: Currently `Show::run()` runs on the main thread forever. The plan requires egui on the main thread (macOS requirement). This means `run_show()` must be restructured: spawn Show into a background thread, run `eframe::run_native` on main. This is the single biggest change — not just "add a field to Show."

2. **`Controller` Send-ability**: `Controller` owns `OscController` and `MidiController`. Moving Show (which owns Controller) to a background thread requires all owned types to be `Send`. `mpsc::Sender`/`Receiver` are `Send`. ZMQ Context is `Send`. MIDI handles via `tunnels::midi` need verification — likely fine but must be checked at compile time.

3. **MIDI rescan is non-trivial**: `MidiController` is constructed once. There's no `replace_midi()` or `rescan()` method. The plan says "if needed" — it IS needed. Options:
   - Add `MidiController::rescan(&mut self)` that drops existing connections and re-discovers
   - Make `Controller.midi` replaceable via `Option<MidiController>` swap
   - **Recommendation**: Defer MIDI rescan to a follow-up. Get the console working with patch editing + OSC first.

4. **`Show::new` already has 8 args**: Adding `Option<ConsoleHandle>` makes 9. The `#[expect(clippy::too_many_arguments)]` already suppresses the warning. Acceptable for now — builder pattern is a future cleanup.

5. **Draft→FixtureGroupConfig serialization**: The plan's approach of building `serde_yaml::Mapping` from key/value pairs and deserializing is correct — `Options` wraps `serde_yaml::Mapping` and `FixtureGroupConfig` uses `#[serde(flatten)]` for options. This will work.

---

## Tool Integration Map

| Tool | Where It Helps | Implementation Phase |
|------|---------------|---------------------|
| **Context7** (egui docs) | `ConsoleApp` layout, egui widgets, `ComboBox`, `CollapsingHeader` patterns | WP-4, WP-5, WP-6 |
| **Context7** (clap docs) | `--console` flag addition to `RunArgs` | WP-3 |
| **Rust Analyzer MCP** | Type checking throughout, especially `Send` bounds on Controller, `Clone` on PatchOption | WP-1, WP-2, WP-3 |
| **GitHub MCP** | Browse `generalelectrix/tunnels` for `MidiController` internals, check `Send` bounds | WP-1 (risk mitigation) |
| **Agent Browser** | Visual verification of egui console window after WP-4+ | WP-4, WP-5, WP-6 (verification) |
| **Art skill** | Architecture diagram of threading model, data flow diagram | Pre-implementation (optional) |
| **CreateCLI skill** | If `--console` flag needs more complex arg handling | WP-3 (if needed) |

---

## Work Packages (Dependency-Ordered)

### Phase 1: Foundation (no egui code, pure data types + wiring)

#### WP-1: Type Prerequisites
**Files**: `src/fixture/patch/option.rs`
**Changes**: Add `#[derive(Clone)]` to `PatchOption`
**Risk check**: Verify `Controller` is `Send` (compile-time check: `fn assert_send<T: Send>() {} assert_send::<Controller>();`)
**Tools**: Rust Analyzer MCP for instant type feedback
**Depends on**: Nothing
**Estimated effort**: Trivial (5 min)

#### WP-2: Console Data Types
**Files**: NEW `src/console/mod.rs`, `src/console/state.rs`, `src/console/command.rs`
**Creates**:
- `ConsoleState` snapshot struct
- `ConsoleCommand` enum
- `ConsoleHandle` (Show side) + `ConsoleAppHandle` (GUI side)
- `run_console()` stub
**Depends on**: WP-1 (needs Clone on PatchOption for FixtureTypeMeta)
**Tools**: Rust Analyzer MCP for struct validation
**Estimated effort**: 20 min

#### WP-3: Show + Main Wiring
**Files**: `src/show.rs`, `src/main.rs`
**Changes**:
- Add `mod console;` to main
- Add `--console` flag to `RunArgs` (use Context7 for clap derive patterns)
- Add `console: Option<ConsoleHandle>` to `Show`
- Update `Show::new` signature
- Add `push_console_state(&self)` and `handle_console_commands(&mut self)` methods
- **Critical**: Restructure `run_show()` — if `--console`, spawn Show thread, run egui on main
- Wire `push_console_state` + `handle_console_commands` into `Show::run()` loop
**Depends on**: WP-2
**Tools**: Context7 (clap), Rust Analyzer MCP (Send bounds)
**Estimated effort**: 45 min (most time on thread inversion)

**Checkpoint**: `cargo build` passes. `cargo run -- run patch/test.yaml --quickstart --console` opens an empty egui window with Show running in background thread.

---

### Phase 2: Read-Only Panels (verify data flow works)

#### WP-4: Current Patch Panel (Read-Only)
**Files**: NEW `src/console/app.rs`
**Creates**: `ConsoleApp` implementing `eframe::App`, central panel showing fixture groups via `CollapsingHeader`
**Depends on**: WP-3
**Tools**: Context7 (egui CollapsingHeader, Panel, Layout), Agent Browser (visual verification)
**Estimated effort**: 30 min

#### WP-5: OSC Clients Panel
**Files**: `src/console/app.rs`
**Adds**: OSC client list display, Add (text input + button) and Remove buttons
**Commands**: `ConsoleCommand::AddOscClient`, `ConsoleCommand::RemoveOscClient`
**Depends on**: WP-4 (need app skeleton)
**Tools**: Context7 (egui TextEdit, Button)
**Estimated effort**: 20 min

#### WP-6: MIDI Devices Panel
**Files**: `src/console/app.rs`
**Adds**: MIDI device list display, Rescan button (sends `ConsoleCommand::RescanMidi`)
**Note**: Rescan command handler is a stub initially — just logs "rescan not yet implemented"
**Depends on**: WP-4
**Tools**: Context7 (egui)
**Estimated effort**: 15 min

**Checkpoint**: Console window shows live fixture patch, OSC clients can be added/removed, MIDI devices listed. Visual verification with Agent Browser.

---

### Phase 3: Patch Editor (the complex part)

#### WP-7: Draft Patch State + Fixture Dropdown
**Files**: `src/console/app.rs`
**Creates**: `DraftPatch`, `DraftGroup`, `DraftPatchBlock` structs. Left panel with fixture type dropdown populated from `PATCHERS`.
**Depends on**: WP-4
**Tools**: Context7 (egui ComboBox)
**Estimated effort**: 25 min

#### WP-8: Dynamic Option Widgets
**Files**: `src/console/app.rs`
**Creates**: Rendering function that maps `PatchOption` variants to egui widgets:
- `Int` → `text_edit_singleline`
- `Bool` → `checkbox`
- `Select(variants)` → `ComboBox`
- `SocketAddr` / `Url` → `text_edit_singleline`
**Depends on**: WP-7
**Tools**: Context7 (egui widget patterns)
**Estimated effort**: 30 min

#### WP-9: Apply Button + Draft→Config Serialization
**Files**: `src/console/app.rs`
**Creates**: Apply button that serializes `DraftPatch` → `Vec<FixtureGroupConfig>` via serde_yaml, sends `ConsoleCommand::Repatch`. Error display in `last_error`.
**Depends on**: WP-8
**Tools**: Rust Analyzer MCP (type checking the serialization path)
**Estimated effort**: 30 min

#### WP-10: Draft Initialization from Live State
**Files**: `src/console/app.rs`
**Creates**: On open and after successful repatch, seed `DraftPatch` from `ConsoleState::groups`
**Depends on**: WP-9
**Estimated effort**: 20 min

**Checkpoint**: Full patch editing works. Edit group options, add/remove patches, Apply → show hot-reloads.

---

### Phase 4: Polish + MIDI Rescan (deferred complexity)

#### WP-11: MIDI Rescan Implementation
**Files**: `src/control.rs`, `src/console/app.rs`
**Changes**: Add ability to replace or rescan MIDI controller. Requires understanding `tunnels::midi` internals.
**Depends on**: WP-6 (stub), all Phase 2 complete
**Tools**: GitHub MCP (browse `generalelectrix/tunnels` repo for MidiController internals)
**Estimated effort**: 45 min (highest uncertainty)

#### WP-12: Error Handling + Edge Cases
**Files**: Various
**Covers**: Universe count exceeded error display, empty patch groups, validation edge cases
**Depends on**: WP-9
**Estimated effort**: 20 min

---

## Parallelization Opportunities

```
WP-1 ──► WP-2 ──► WP-3 ──► WP-4 ──┬──► WP-5
                                     ├──► WP-6
                                     └──► WP-7 ──► WP-8 ──► WP-9 ──► WP-10
                                                                        │
WP-6 stub ──────────────────────────────────────────────────────► WP-11
WP-9 ──────────────────────────────────────────────────────────► WP-12
```

- **WP-5 and WP-6** can run in parallel (both depend only on WP-4)
- **WP-7** can start as soon as WP-4 is done (parallel with WP-5/WP-6)
- **WP-11** is independent and can be deferred entirely
- **WP-12** can run after WP-9

**Potential agent parallelization**: After WP-4, spawn separate agents for WP-5, WP-6, and WP-7.

---

## Verification Strategy

| Check | Command/Method | When |
|-------|---------------|------|
| Compiles | `cargo build` | After each WP |
| Tests pass | `cargo test` | After WP-1, WP-2, WP-3 |
| Console opens | `cargo run -- run patch/test.yaml --quickstart --console` | After WP-3 |
| Patch display | Agent Browser screenshot | After WP-4 |
| OSC add/remove | Manual test via console | After WP-5 |
| Hot reload | Edit patch in console, Apply, observe DMX changes | After WP-9 |
| Error display | Apply invalid patch, observe red error | After WP-12 |
| Type safety | Rust Analyzer MCP continuous | All WPs |
| egui patterns | Context7 docs queries | WP-4 through WP-10 |

---

## Key Decisions

1. **MIDI rescan deferred to WP-11** — Get the core console working first. The rescan button will exist but show "not yet implemented" until WP-11.
2. **Thread inversion is the critical path** — WP-3 is the hardest work package because it restructures the main thread model. Everything after it is incremental.
3. **Draft serialization via serde_yaml** — The plan's approach works because `FixtureGroupConfig` uses `#[serde(flatten)]` for `Options`. Building a `Mapping` from key/value strings and deserializing is the right approach.
4. **No builder pattern for Show::new yet** — 9 args is ugly but functional. Refactor separately if desired.
