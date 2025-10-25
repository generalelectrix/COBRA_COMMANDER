//! Types and traits for specification of patch options.
use std::{fmt::Display, net::SocketAddr};

use anyhow::Error;
use serde::Deserialize;
use strum::IntoEnumIterator;
use url::Url;

use crate::{config::Options, fixture::fixture::EnumRenderModel};
/// A type that fixtures can use to declare that they do not accept patch options.
#[derive(Deserialize)]
#[serde(try_from = "ParseNoOptions")]
pub struct NoOptions {}

/// Deserialize as this helper type so we can provide a nice error message.
#[derive(Deserialize)]
struct ParseNoOptions(Options);

impl TryFrom<ParseNoOptions> for NoOptions {
    type Error = Error;
    fn try_from(value: ParseNoOptions) -> std::result::Result<Self, Self::Error> {
        value.0.ensure_empty()?;
        Ok(Self {})
    }
}

pub trait OptionsMenu {
    fn menu() -> Vec<(String, PatchOption)>;
}

impl OptionsMenu for NoOptions {
    fn menu() -> Vec<(String, PatchOption)> {
        vec![]
    }
}

pub trait AsPatchOption {
    fn as_patch_option() -> PatchOption;
}

impl AsPatchOption for usize {
    fn as_patch_option() -> PatchOption {
        PatchOption::Int
    }
}

impl AsPatchOption for SocketAddr {
    fn as_patch_option() -> PatchOption {
        PatchOption::SocketAddr
    }
}

impl AsPatchOption for Url {
    fn as_patch_option() -> PatchOption {
        PatchOption::Url
    }
}

impl AsPatchOption for bool {
    fn as_patch_option() -> PatchOption {
        PatchOption::Bool
    }
}

// TODO: make optionality explicit
impl<T: AsPatchOption> AsPatchOption for Option<T> {
    fn as_patch_option() -> PatchOption {
        T::as_patch_option()
    }
}

/// Create a patch option for an iterable enum.
pub fn enum_patch_option<T: IntoEnumIterator + Display>() -> PatchOption {
    PatchOption::Select(T::iter().map(|x| x.to_string()).collect())
}

/// Blanket-derive for enum render models.
impl<T: EnumRenderModel + IntoEnumIterator + Display> AsPatchOption for T {
    fn as_patch_option() -> PatchOption {
        enum_patch_option::<Self>()
    }
}

/// The kinds of patch options that fixtures can specify.
pub enum PatchOption {
    /// An integer.
    Int,
    /// Select a specific option from a menu.
    Select(Vec<String>),
    /// A network address.
    SocketAddr,
    /// A URL.
    Url,
    /// A boolean option.
    Bool,
}

impl Display for PatchOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int => f.write_str("<integer>"),
            Self::Select(opts) => f.write_str(&opts.join(", ")),
            Self::SocketAddr => f.write_str("<socket address>"),
            Self::Url => f.write_str("<url>"),
            Self::Bool => f.write_str("true, false"),
        }
    }
}
