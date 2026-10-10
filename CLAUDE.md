# Cobra Commander

## No panics in production code
Production code must never panic. This means:
- No `.unwrap()` or `.expect()` on `Option` or `Result` — use `let Some(x) = ... else { return }`, `.unwrap_or()`, `.get()`, or propagate errors via `?`
- No unchecked array/slice indexing `[i]` — use `.get(i)`
- No `panic!()`, `unreachable!()`, or `todo!()`
- This is a live performance lighting controller — a crash during a show is unacceptable

## Spawn long-lived threads via `crate::worker`
Always use `crate::worker::spawn("name", |shutdown| …)`, never `std::thread::spawn`. This registers
the handle so the app signals and joins every worker on quit (releasing sockets/ports). In loops,
poll `shutdown.triggered()` or wait with `shutdown.sleep_or_shutdown(dur)`.

## Never force the GUI to repaint continuously to keep a value fresh
State the GUI shows goes through a `Notified`/`NotifiedAtomic` field of `GuiState` (`src/gui_state.rs`),
stored only when the shown value changes (`NotifiedAtomic::store_if_changed`); the store wakes the GUI. A value
produced on a real-time thread (e.g. the audio input's clip LED and trim) is relayed by the show tick,
since the audio thread must never fire the repaint signal. Continuous repaint is only for a visible,
genuinely animating view (an open envelope viewer, the animation visualizer) when there is no other option.
