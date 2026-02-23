# COBRA_COMMANDER

DMX lighting controller written in Rust. Controls stage lights via DMX, with OSC
input, MIDI integration, and WLED LED strip control.

## Architecture

- `src/main.rs` — CLI entry point (clap)
- `src/show.rs` — Show state management (15K, core)
- `src/fixture/` — Fixture type definitions (9 subdirs)
- `src/osc/` — OSC control input handlers (17 subdirs)
- `src/midi/` — MIDI control handlers (5 subdirs)
- `src/dmx.rs` — DMX universe output
- `src/animation.rs` — Animation/effect system
- `src/strobe.rs` — Strobe effects (14K)
- `src/color.rs` — Color handling (11K)
- `src/wled.rs` — WLED LED strip integration
- `fixture_macros/` — Proc-macro crate for fixture definitions

## Key Dependencies

- `rosc` — OSC protocol
- `rust_dmx` — DMX output
- `tunnels` / `tunnels_lib` — Color organ effects (git dep from generalelectrix/tunnels)
- `color_organ` — Color organ (git dep from generalelectrix/color_organ)
- `egui` / `eframe` — GUI visualization / animation_visualizer
- `zmq` — ZeroMQ messaging (tunnels communication)
- `wled-json-api-library` + `reqwest` — WLED HTTP API
- `clap` — CLI argument parsing
- `linkme` — Distributed slice for fixture registration

## Build & Run

```bash
cargo build
cargo run -- [options]
```

## Patterns

- Fixtures use `#[linkme]` distributed slice + proc macros for auto-registration
- OSC and MIDI handlers are organized by fixture/subsystem in subdirs
- `enum_dispatch` used for fixture trait dispatch
- YAML config files for show/fixture setup
