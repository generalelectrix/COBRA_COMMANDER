//! Define patchers for fixture types.
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::fmt::{Display, Write};

use super::{OptionsMenu, PatchOption};
use crate::config::{FixtureGroupKey, Options};
use crate::fixture::fixture::{
    AnimatedFixture, FixtureType, FixtureWithAnimations, NonAnimatedFixture, RenderMode,
};
use crate::fixture::group::FixtureGroup;
use crate::strobe::StrobeResponse;
use linkme::distributed_slice;

/// Distributed registry for things that we can patch.
///
/// The derive macros for the patch traits handle this.
/// Use the register_patcher macro for fixtures that cannot derive patch.
#[distributed_slice]
pub static PATCHERS: [Patcher];

#[derive(Clone)]
pub struct Patcher {
    pub name: FixtureType,
    pub create_group: fn(FixtureGroupKey, Options) -> Result<FixtureGroup>,
    pub group_options: fn() -> Vec<(String, PatchOption)>,
    pub create_patch: fn(group_options: Options, patch_options: Options) -> Result<PatchConfig>,
    pub patch_options: fn() -> Vec<(String, PatchOption)>,
}

impl Display for Patcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;

        let patch_opts = (self.patch_options)();
        let group_opts = (self.group_options)();
        // If a fixture doesn't take options, we should be able to get a channel count.
        // TODO: we should make it possible to generate patch configs for all
        // enumerable options
        if patch_opts.is_empty() && group_opts.is_empty() {
            if let Ok(fix) = (self.create_patch)(Default::default(), Default::default()) {
                if fix.channel_count > 0 {
                    write!(
                        f,
                        " ({} channel{})",
                        fix.channel_count,
                        if fix.channel_count > 1 { "s" } else { "" }
                    )?;
                }
            }
        }

        if patch_opts.is_empty() && group_opts.is_empty() {
            return Ok(());
        }
        f.write_char('\n')?;
        if !group_opts.is_empty() {
            writeln!(f, "  group-level options:")?;
        }

        for (key, opt) in group_opts {
            writeln!(f, "    {key}: {opt}")?;
        }

        if !patch_opts.is_empty() {
            writeln!(f, "  patch-level options:")?;
        }

        for (key, opt) in patch_opts {
            writeln!(f, "    {key}: {opt}")?;
        }
        Ok(())
    }
}

/// A configuration for a single fixture to be patched in a group.
pub struct PatchConfig {
    pub channel_count: usize,
    pub render_mode: Option<RenderMode>,
}

pub trait PatchFixture: Sized + 'static {
    const NAME: FixtureType;

    type GroupOptions;
    type PatchOptions;

    /// Return the menu of group-level options for this fixture type.
    fn group_options() -> Vec<(String, PatchOption)>
    where
        <Self as PatchFixture>::GroupOptions: OptionsMenu,
    {
        Self::GroupOptions::menu()
    }

    /// Create a new instance of the fixture from parsed options.
    fn new(options: Self::GroupOptions) -> Self;

    /// Parse and validate group options.
    fn parse_group_options(options: Options) -> Result<Self::GroupOptions>
    where
        <Self as PatchFixture>::GroupOptions: DeserializeOwned,
    {
        options.parse().context("group options")
    }

    /// Parse options and create a new instance of this fixture.
    fn create(options: Options) -> Result<Self>
    where
        <Self as PatchFixture>::GroupOptions: DeserializeOwned,
    {
        Ok(Self::new(Self::parse_group_options(options)?))
    }

    /// If this fixture can strobe, return its response profile.
    fn can_strobe() -> Option<StrobeResponse> {
        None
    }

    /// Parse and validate patch options.
    fn parse_patch_options(options: Options) -> Result<Self::PatchOptions>
    where
        <Self as PatchFixture>::PatchOptions: DeserializeOwned,
    {
        options.parse().context("patch options")
    }

    /// Parse options and create a patch config.
    fn create_patch(group_options: Options, patch_options: Options) -> Result<PatchConfig>
    where
        <Self as PatchFixture>::GroupOptions: DeserializeOwned,
        <Self as PatchFixture>::PatchOptions: DeserializeOwned,
    {
        Ok(Self::new_patch(
            Self::parse_group_options(group_options)?,
            Self::parse_patch_options(patch_options)?,
        ))
    }

    /// Given group- and patch-level options, produce a patch config.
    fn new_patch(
        group_options: Self::GroupOptions,
        patch_options: Self::PatchOptions,
    ) -> PatchConfig;

    /// Return the menu of patch options for this fixture type.
    fn patch_options() -> Vec<(String, PatchOption)>
    where
        <Self as PatchFixture>::PatchOptions: OptionsMenu,
    {
        Self::PatchOptions::menu()
    }
}

/// Create a fixture group for a non-animated fixture.
pub trait CreateNonAnimatedGroup: PatchFixture + NonAnimatedFixture + Sized + 'static {
    /// Create an empty fixture group for this type of fixture.
    fn create_group(key: FixtureGroupKey, options: Options) -> Result<FixtureGroup>
    where
        <Self as PatchFixture>::GroupOptions: DeserializeOwned,
    {
        let fixture = Box::new(Self::create(options.clone())?);
        Ok(FixtureGroup::empty(
            Self::NAME,
            key,
            fixture,
            Self::can_strobe(),
            options,
        ))
    }
}

impl<T> CreateNonAnimatedGroup for T where T: PatchFixture + NonAnimatedFixture + Sized + 'static {}

/// Create a fixture group for an animated fixture.
pub trait CreateAnimatedGroup: PatchFixture + AnimatedFixture + Sized + 'static {
    /// Create an empty fixture group for this type of fixture.
    fn create_group(key: FixtureGroupKey, options: Options) -> Result<FixtureGroup>
    where
        <Self as PatchFixture>::GroupOptions: DeserializeOwned,
    {
        let fixture = Self::create(options.clone())?;
        Ok(FixtureGroup::empty(
            Self::NAME,
            key,
            Box::new(FixtureWithAnimations {
                fixture,
                animations: Default::default(),
            }),
            Self::can_strobe(),
            options,
        ))
    }
}

impl<T> CreateAnimatedGroup for T where T: PatchFixture + AnimatedFixture + Sized + 'static {}
