# Plan: Step 5 — Dynamic DMX Port Assignment

## Context

Steps 1-4.5 of the dynamic configuration refactoring are complete. The command-response infrastructure (`CommandClient`, `offer_action`, CLI config thread) is in place and proven with the animation visualizer.

Step 5 moves DMX port assignment from the pre-show startup path into the CLI configuration thread. Currently, non-quickstart users are prompted to assign each universe before the show starts. After this change, the show starts immediately with offline ports, and the CLI thread prompts the user to assign real ports against the running show.

## Design

The CLI thread calls `available_ports()`, prompts the user, and sends the selected `Box<dyn DmxPort>` (unopened) to the show via `MetaCommand::AssignDmxPort`. The show calls `port.open()` and swaps it in, reporting success/failure via the response channel.

No `DmxPortSpec` abstraction — `DmxPort: Send` (fixed in `rust_dmx` 0.7) allows passing trait objects directly through the mpsc channel.

## Changes

### 1. `src/control.rs` — Add `AssignDmxPort` variant, drop `Clone` from `MetaCommand`

Drop `Clone` derive from `MetaCommand` (nothing clones it; `ControlMessage` itself isn't `Clone`):

```rust
#[derive(Debug)]
pub enum MetaCommand {
    ReloadPatch,
    RefreshUI,
    ResetAllAnimations,
    StartAnimationVisualizer,
    AssignDmxPort {
        universe: usize,
        port: Box<dyn DmxPort>,
    },
}
```

`DmxPort` is `Display + Send` but not `Debug`. Since `MetaCommand` derives `Debug`, we need a manual `Debug` impl. Replace `#[derive(Debug)]` with a hand-written impl that formats `AssignDmxPort` using the port's `Display` impl:

```rust
impl fmt::Debug for MetaCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReloadPatch => write!(f, "ReloadPatch"),
            Self::RefreshUI => write!(f, "RefreshUI"),
            Self::ResetAllAnimations => write!(f, "ResetAllAnimations"),
            Self::StartAnimationVisualizer => write!(f, "StartAnimationVisualizer"),
            Self::AssignDmxPort { universe, port } => {
                f.debug_struct("AssignDmxPort")
                    .field("universe", universe)
                    .field("port", &format_args!("{port}"))
                    .finish()
            }
        }
    }
}
```

Add `use rust_dmx::DmxPort;` import to control.rs.

### 2. `src/show.rs` — Handle `AssignDmxPort` in `handle_meta_command()`

Add match arm:

```rust
MetaCommand::AssignDmxPort { universe, mut port } => {
    if universe >= self.dmx_ports.len() {
        bail!(
            "universe {universe} out of range (show has {} universe(s))",
            self.dmx_ports.len()
        );
    }
    port.open().map_err(|e| anyhow::anyhow!("failed to open port {port}: {e}"))?;
    self.dmx_buffers[universe].fill(0);
    self.dmx_ports[universe] = port;
    Ok(())
}
```

The DMX buffer is zeroed before swapping to avoid writing stale data through the new port. `open()` failure produces a descriptive error including the port's Display name.

### 3. `src/main.rs` — Remove non-quickstart DMX prompt, pass `universe_count` to CLI thread

Remove the non-quickstart branch of DMX port assignment. Non-quickstart starts with all offline ports:

```rust
let universe_count = patch.universe_count();
println!("This show requires {universe_count} universe(s).");

let dmx_ports: Vec<Box<dyn DmxPort>> = if args.quickstart {
    // Quickstart auto-assigns available ports, filling remainder with offline.
    let mut ports = Vec::new();
    let mut available = available_ports(args.artnet.then_some(ARTNET_POLL_TIMEOUT))?;
    for (i, port) in (0..universe_count).zip(available.into_iter().rev().chain(
        std::iter::repeat_with(|| Box::new(OfflineDmxPort) as Box<dyn DmxPort>),
    )) {
        println!("Assigning universe {i} to port {port}.");
        ports.push(port);
    }
    ports
} else {
    (0..universe_count)
        .map(|_| Box::new(OfflineDmxPort) as Box<dyn DmxPort>)
        .collect()
};
```

Pass `universe_count` and `artnet` flag to the CLI thread:

```rust
if !args.quickstart {
    let cli_client = command_client.clone();
    let artnet = args.artnet;
    std::thread::spawn(move || {
        if let Err(e) = cli::run_cli_configuration(cli_client, universe_count, artnet) {
            error!("CLI configuration error: {e:#}");
        }
    });
}
```

Remove unused imports: `select_port_from`.

### 4. `src/cli.rs` — Add `prompt_assign_dmx_ports()`, update `run_cli_configuration` signature

Update `run_cli_configuration` to accept context parameters:

```rust
pub(crate) fn run_cli_configuration(
    client: CommandClient,
    universe_count: usize,
    artnet: bool,
) -> Result<()> {
    offer_action(&client, |c| prompt_assign_dmx_ports(c, universe_count, artnet))?;
    offer_action(&client, prompt_start_animation_visualizer)?;
    Ok(())
}
```

The `offer_action` signature changes from `fn` pointer to a closure (or we keep `fn` and use a different approach). Since `prompt_assign_dmx_ports` needs extra parameters, `offer_action` should accept `impl Fn` instead of `fn`:

```rust
fn offer_action(
    client: &CommandClient,
    action: impl Fn(&CommandClient) -> Result<Option<CommandResponse>>,
) -> Result<()> {
    // body unchanged
}
```

Add DMX port assignment prompts. We can't use `select_port_from()` because it calls `port.open()` before returning — the show should open ports so it can report errors. Write our own `prompt_select_port()` using `tunnels_lib::prompt::prompt_parse`:

```rust
use rust_dmx::{DmxPort, available_ports, OfflineDmxPort};
use tunnels_lib::prompt::prompt_parse;
use std::time::Duration;

const ARTNET_POLL_TIMEOUT: Duration = Duration::from_secs(10);

fn prompt_assign_dmx_ports(
    client: &CommandClient,
    universe_count: usize,
    artnet: bool,
) -> Result<Option<CommandResponse>> {
    let artnet_timeout = artnet.then_some(ARTNET_POLL_TIMEOUT);
    if artnet {
        println!("Searching for artnet ports...");
    }
    let mut ports = available_ports(artnet_timeout)?;
    for universe in 0..universe_count {
        println!("Assign port to universe {universe}:");
        let port = prompt_select_port(&mut ports)?;
        let response = client.send_command(MetaCommand::AssignDmxPort { universe, port })?;
        if let Err(e) = response {
            println!("Error assigning universe {universe}: {e}");
        }
    }
    Ok(Some(Ok(())))
}

/// Prompt the user to select a DMX port. Does NOT open the port.
fn prompt_select_port(ports: &mut Vec<Box<dyn DmxPort>>) -> Result<Box<dyn DmxPort>> {
    println!("Available DMX ports:");
    println!("0: offline");
    for (i, port) in ports.iter().enumerate() {
        println!("{}: {port}", i + 1);
    }
    prompt_parse("Select a port", |input| {
        let index: usize = input.parse()?;
        if index == 0 {
            return Ok(Box::new(OfflineDmxPort) as Box<dyn DmxPort>);
        }
        let index = index - 1;
        if index >= ports.len() {
            bail!("please enter a value less than {}", ports.len() + 1);
        }
        Ok(ports.remove(index))
    })
}
```

### 5. Testing Strategy

**New file: `src/dmx.rs` additions (or inline in tests)**

Create a `MockDmxPort` for testing the `AssignDmxPort` handler. Place it in `src/dmx.rs` under `#[cfg(test)]` since that's where DMX types live, or in a test module within `show.rs`.

```rust
#[cfg(test)]
pub mod mock {
    use rust_dmx::{DmxPort, OpenError, WriteError};
    use serde::{Deserialize, Serialize};
    use std::fmt;

    #[derive(Serialize, Deserialize)]
    pub struct MockDmxPort {
        pub open_should_fail: bool,
        pub opened: bool,
        pub frames_written: usize,
    }

    impl MockDmxPort {
        pub fn new() -> Self {
            Self {
                open_should_fail: false,
                opened: false,
                frames_written: 0,
            }
        }

        pub fn failing() -> Self {
            Self {
                open_should_fail: true,
                opened: false,
                frames_written: 0,
            }
        }
    }

    #[typetag::serde]
    impl DmxPort for MockDmxPort {
        fn open(&mut self) -> Result<(), OpenError> {
            if self.open_should_fail {
                Err(OpenError::NotConnected)
            } else {
                self.opened = true;
                Ok(())
            }
        }

        fn close(&mut self) {
            self.opened = false;
        }

        fn write(&mut self, _frame: &[u8]) -> Result<(), WriteError> {
            if !self.opened {
                return Err(WriteError::Disconnected);
            }
            self.frames_written += 1;
            Ok(())
        }
    }

    impl fmt::Display for MockDmxPort {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "mock")
        }
    }
}
```

**Note on `#[typetag::serde]`:** The `DmxPort` trait uses `#[typetag::serde(tag = "type")]`, so every impl needs `#[typetag::serde]` and must derive/impl `Serialize` + `Deserialize`. Test impls included.

**Tests to write** (in `src/show.rs` or `src/control.rs` test modules):

1. **`assign_dmx_port_success`** — Send `AssignDmxPort` with a `MockDmxPort`. Verify the port was opened, the buffer was zeroed, and the response is `Ok(())`.

2. **`assign_dmx_port_open_fails`** — Send `AssignDmxPort` with a `MockDmxPort::failing()`. Verify the response contains the open error and the original port is unchanged.

3. **`assign_dmx_port_universe_out_of_range`** — Send `AssignDmxPort` with `universe` >= port count. Verify the response is an error mentioning "out of range".

4. **`meta_command_debug_formats_port_display`** — Verify the manual `Debug` impl on `MetaCommand::AssignDmxPort` produces readable output using the port's `Display`.

**Testing approach for handler tests:** The tests need to exercise `Show::handle_meta_command()`, but `Show::new()` requires a full `Controller` with OSC/MIDI setup. Two options:

- **Option A:** Test `handle_meta_command` via the full `Show::control()` path using the channel — requires constructing a `Show` which needs a `Controller` (involves real OSC listener bind). Heavy.
- **Option B:** Extract the `AssignDmxPort` logic into a standalone function that takes `&mut Vec<Box<dyn DmxPort>>` and `&mut Vec<DmxBuffer>`. Test that function directly. Lighter, more focused.

**Recommendation: Option B** — Extract a `fn assign_dmx_port(ports: &mut ..., buffers: &mut ..., universe: usize, port: Box<dyn DmxPort>) -> Result<()>` and test it directly. The handler just delegates. This matches how other handler logic could be extracted for testing without needing the full Show.

## Files Modified

| File | Change |
|------|--------|
| `src/control.rs` | Add `AssignDmxPort` variant, drop `Clone`, manual `Debug` impl, add `rust_dmx::DmxPort` import |
| `src/show.rs` | Add `AssignDmxPort` match arm in `handle_meta_command()`, extract testable function |
| `src/main.rs` | Remove non-quickstart DMX prompt, start with offline ports, pass `universe_count`+`artnet` to CLI thread |
| `src/cli.rs` | Add `prompt_assign_dmx_ports()`, `prompt_select_port()`, update `run_cli_configuration` signature, change `offer_action` to accept closures |
| `src/dmx.rs` | Add `#[cfg(test)] mod mock` with `MockDmxPort` |

## Verification

1. `cargo check -j 2` — compiles cleanly
2. `cargo clippy -j 2` — no new warnings
3. `cargo test -j 2` — new tests pass
4. Test: `assign_dmx_port_success` — mock port opened and swapped in
5. Test: `assign_dmx_port_open_fails` — error reported, original port unchanged
6. Test: `assign_dmx_port_universe_out_of_range` — error reported
7. `--quickstart` still auto-assigns ports (unchanged behavior)
8. Non-quickstart starts with offline ports, CLI thread prompts for assignment
