//! Cobra Commander internals exposed for `examples/` binaries.
//!
//! The production entry point is `src/main.rs`; this library surface exists
//! only so that throwaway example binaries can construct and exercise the
//! color-conversion code without duplicating it.
//!
//! `src/color.rs` derives `AsPatchOption` from `fixture_macros`, which
//! expands to a reference to `crate::fixture::patch::{AsPatchOption,
//! PatchOption, enum_patch_option}`. To keep the lib build self-contained,
//! we provide no-op stubs at those paths. The binary continues to use the
//! real implementations under `src/fixture/patch/`.

pub mod fixture {
    pub mod patch {
        use std::fmt::Display;
        use strum::IntoEnumIterator;

        pub struct PatchOption;

        pub trait AsPatchOption {
            fn as_patch_option() -> PatchOption;
        }

        pub fn enum_patch_option<T: IntoEnumIterator + Display>() -> PatchOption {
            PatchOption
        }
    }
}

pub mod color;
