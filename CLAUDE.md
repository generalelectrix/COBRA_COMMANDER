# Cobra Commander

## No panics in production code
Production code must never panic. This means:
- No `.unwrap()` or `.expect()` on `Option` or `Result` — use `let Some(x) = ... else { return }`, `.unwrap_or()`, `.get()`, or propagate errors via `?`
- No unchecked array/slice indexing `[i]` — use `.get(i)`
- No `panic!()`, `unreachable!()`, or `todo!()`
- This is a live performance lighting controller — a crash during a show is unacceptable
