use std::fmt::Debug;
use std::fmt::Display;
use std::marker::PhantomData;

use anyhow::bail;
use num_traits::FromPrimitive;
use num_traits::ToPrimitive;
use strum::IntoEnumIterator;
use tunnels::animation::Animation;

/// The number of animators to use for each group.
pub const N_ANIM: usize = 4;
pub type TargetedAnimations<T> = [TargetedAnimation<T>; N_ANIM];

/// Numeric index for an animation target.
/// This is used to represent an animation target as a generic selection.
pub type AnimationTargetIndex = usize;

/// A source of (animation_value, target) pairs that consumers can iterate
/// without caring about the underlying storage.
///
/// Implementations include [`AnimationSlice`] (the leaf, backed by a stack
/// buffer materialized in `FixtureWithAnimations::render`) and
/// [`SubtargetView`] (a lazy projection produced by [`Self::subtarget`]).
///
/// `filter`, `all`, and `subtarget` are derived from `iter` and apply lazily,
/// so chaining them never materializes an intermediate buffer. Static
/// dispatch (generic trait params at call sites) keeps the hot render path
/// allocation-free.
pub trait TargetedAnimationValues<T>
where
    T: PartialEq + Copy,
{
    /// Iterate over the (value, target) pairs in this source.
    fn iter(&self) -> impl Iterator<Item = (f64, T)>;

    /// Iterate over all of the animation values, regardless of target.
    fn all(&self) -> impl Iterator<Item = f64> {
        self.iter().map(|(v, _)| v)
    }

    /// Iterate over all animation values matching the provided target.
    fn filter<'a>(&'a self, target: &'a T) -> impl Iterator<Item = f64> + 'a {
        self.iter()
            .filter_map(move |(v, t)| (t == *target).then_some(v))
    }

    /// Lazily project this source down to a subtarget type. The returned
    /// view borrows `self`; iterating it walks the source and applies the
    /// supertarget→subtarget projection on the fly. Entries whose targets
    /// don't map are skipped.
    fn subtarget<U>(&self) -> SubtargetView<'_, Self, U, T>
    where
        U: PartialEq + Copy + FromSupertarget<T>,
        Self: Sized,
    {
        SubtargetView(self, PhantomData)
    }
}

/// Leaf source: a borrowed slice of (value, target) pairs. Typically the slice
/// is materialized on the caller's stack in `FixtureWithAnimations::render`.
pub struct AnimationSlice<'a, T>(pub &'a [(f64, T)]);

impl<'a, T> TargetedAnimationValues<T> for AnimationSlice<'a, T>
where
    T: PartialEq + Copy,
{
    fn iter(&self) -> impl Iterator<Item = (f64, T)> {
        self.0.iter().map(|(v, t)| (*v, *t))
    }
}

/// Lazy projection from supertarget type `T` to subtarget type `U`. Created by
/// [`TargetedAnimationValues::subtarget`]. Iterating it walks the underlying
/// source and applies [`FromSupertarget`]; entries that don't map are dropped.
pub struct SubtargetView<'a, S: ?Sized, U, T>(&'a S, PhantomData<(U, T)>);

impl<'a, S, T, U> TargetedAnimationValues<U> for SubtargetView<'a, S, U, T>
where
    S: TargetedAnimationValues<T> + ?Sized,
    T: PartialEq + Copy,
    U: PartialEq + Copy + FromSupertarget<T>,
{
    fn iter(&self) -> impl Iterator<Item = (f64, U)> {
        self.0
            .iter()
            .filter_map(|(v, t)| U::from_supertarget(&t).map(|u| (v, u)))
    }
}

/// A pairing of an animation and a target.
#[derive(Debug, Clone, Default)]
pub struct TargetedAnimation<T: AnimationTarget> {
    pub animation: Animation,
    pub target: T,
}

/// An animation target should be an enum with a unit variant for each option.
pub trait AnimationTarget:
    ToPrimitive
    + FromPrimitive
    + IntoEnumIterator
    + Display
    + Clone
    + Copy
    + Default
    + Debug
    + PartialEq
{
}

impl<T> AnimationTarget for T where
    T: ToPrimitive
        + FromPrimitive
        + IntoEnumIterator
        + Display
        + Clone
        + Copy
        + Default
        + Debug
        + PartialEq
{
}

/// Interface to a targeted animation.
/// Targets are handled as numeric indices.
pub trait ControllableTargetedAnimation {
    /// Get an immutable reference to the inner animation.
    fn anim(&self) -> &Animation;
    /// Get a mutable reference to the inner animation.
    fn anim_mut(&mut self) -> &mut Animation;
    /// Get the current animation target as an index.
    fn target(&self) -> AnimationTargetIndex;
    /// Set the current animation target to the provided index.
    /// Return an error if the index is invalid for this target type.
    fn set_target(&mut self, index: AnimationTargetIndex) -> anyhow::Result<()>;
    /// Return the labels for the animation target type.
    fn target_labels(&self) -> Vec<String>;
    /// Reset the state of this animation to default.
    fn reset(&mut self);
}

impl<T: AnimationTarget> ControllableTargetedAnimation for TargetedAnimation<T> {
    fn anim(&self) -> &Animation {
        &self.animation
    }

    fn anim_mut(&mut self) -> &mut Animation {
        &mut self.animation
    }

    fn target(&self) -> AnimationTargetIndex {
        self.target.to_usize().unwrap()
    }

    fn set_target(&mut self, index: AnimationTargetIndex) -> anyhow::Result<()> {
        let Some(target) = T::from_usize(index) else {
            bail!(
                "animation index {index} out of range for {}",
                std::any::type_name::<T>()
            );
        };
        self.target = target;
        Ok(())
    }

    fn target_labels(&self) -> Vec<String> {
        T::iter().map(|t| t.to_string()).collect()
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Helper trait for fixtures that embed another type of fixture as a control.
/// The animation target type can define this trait, and then targeted animation
/// values can be filtered down to be passed to the subfixture.
///
/// Implementing this trait automatically implements the other direction as a
/// method on the target type.
pub trait Subtarget<T> {
    fn as_subtarget(&self) -> Option<T>;
}

pub trait FromSupertarget<T> {
    fn from_supertarget(supertarget: &T) -> Option<Self>
    where
        Self: std::marker::Sized;
}

impl<T, U> FromSupertarget<T> for U
where
    T: Subtarget<U>,
{
    fn from_supertarget(supertarget: &T) -> Option<Self>
    where
        Self: std::marker::Sized,
    {
        supertarget.as_subtarget()
    }
}
