//! Types and traits for specification of patch options.
use std::{fmt::Display, net::SocketAddr};

use anyhow::Error;
use number::BipolarFloat;
use serde::{Deserialize, Deserializer, de};
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

impl AsPatchOption for u8 {
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

impl AsPatchOption for BipolarFloat {
    fn as_patch_option() -> PatchOption {
        PatchOption::Bipolar
    }
}

/// Deserialize a [`BipolarFloat`], erroring if the value is outside [-1, 1].
///
/// `BipolarFloat`'s derived `Deserialize` reads its inner float directly,
/// bypassing the clamp that normally upholds its range invariant. Use this for
/// config fields that should reject an out-of-range value up front rather than
/// silently accept or coerce it.
pub fn deserialize_bipolar<'de, D>(deserializer: D) -> Result<BipolarFloat, D::Error>
where
    D: Deserializer<'de>,
{
    let v = f64::deserialize(deserializer)?;
    if !(-1.0..=1.0).contains(&v) {
        return Err(de::Error::custom(format!(
            "value {v} is out of range [-1, 1]"
        )));
    }
    Ok(BipolarFloat::new(v))
}

impl<T: AsPatchOption> AsPatchOption for Option<T> {
    fn as_patch_option() -> PatchOption {
        // Recursively unwrap any nested Optional to a single Optional(leaf).
        // Bool is special-cased: Option<bool> stays as Bool since a checkbox
        // naturally represents absent/false without needing a three-state widget.
        fn flatten(opt: PatchOption) -> PatchOption {
            match opt {
                PatchOption::Optional(inner) => flatten(*inner),
                PatchOption::Bool => PatchOption::Bool,
                other => PatchOption::Optional(Box::new(other)),
            }
        }
        flatten(T::as_patch_option())
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
    /// A bipolar float in the range [-1, 1].
    Bipolar,
    /// An optional value. The inner type describes the value when present.
    Optional(Box<PatchOption>),
}

#[cfg(test)]
impl PatchOption {
    pub fn example_value(&self) -> serde_yaml::Value {
        use serde_yaml::Value;
        match self {
            PatchOption::Int => Value::Number(1.into()),
            PatchOption::Bool => Value::Bool(false),
            PatchOption::Bipolar => Value::Number(0.0.into()),
            PatchOption::Select(opts) => Value::String(opts[0].clone()),
            PatchOption::SocketAddr => Value::String("127.0.0.1:9999".into()),
            PatchOption::Url => Value::String("http://127.0.0.1:9999".into()),
            PatchOption::Optional(inner) => inner.example_value(),
        }
    }
}

impl Display for PatchOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int => f.write_str("<integer>"),
            Self::Select(opts) => f.write_str(&opts.join(", ")),
            Self::SocketAddr => f.write_str("<socket address>"),
            Self::Url => f.write_str("<url>"),
            Self::Bool => f.write_str("true, false"),
            Self::Bipolar => f.write_str("<bipolar float, -1..1>"),
            Self::Optional(inner) => write!(f, "{inner} (optional)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Wrap(#[serde(deserialize_with = "deserialize_bipolar")] BipolarFloat);

    #[test]
    fn bipolar_as_patch_option() {
        assert!(matches!(
            BipolarFloat::as_patch_option(),
            PatchOption::Bipolar
        ));
    }

    #[test]
    fn deserialize_bipolar_in_range() {
        let Wrap(v) = serde_yaml::from_str("0.33").unwrap();
        assert!((v.val() - 0.33).abs() < 1e-9);

        // Boundaries are inclusive.
        let Wrap(v) = serde_yaml::from_str("-1.0").unwrap();
        assert_eq!(v, BipolarFloat::new(-1.0));
    }

    #[test]
    fn deserialize_bipolar_rejects_out_of_range() {
        let err = serde_yaml::from_str::<Wrap>("1.5").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("out of range") && msg.contains("1.5"),
            "unexpected error message: {msg}"
        );
    }
}
