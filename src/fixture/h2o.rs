//! Intuitive control profile for the American DJ H2O DMX Pro.

use anyhow::Context;
use num_derive::{FromPrimitive, ToPrimitive};
use number::{BipolarFloat, UnipolarFloat};

use super::prelude::*;
use crate::util::bipolar_to_split_range;
use crate::util::unipolar_to_range;
use strum_macros::{Display as EnumDisplay, EnumIter, EnumString};

#[derive(Default, Debug)]
pub struct H2O {
    dimmer: UnipolarFloat,
    rotation: BipolarFloat,
    fixed_color: FixedColor,
    color_rotate: bool,
    color_rotation: BipolarFloat,
}

impl PatchAnimatedFixture for H2O {
    const NAME: FixtureType = FixtureType("h2o");
    fn channel_count(&self) -> usize {
        3
    }
}

impl H2O {
    fn handle_state_change(
        &mut self,
        sc: StateChange,
        emitter: &mut dyn crate::osc::EmitControlMessage,
    ) {
        use StateChange::*;
        match sc {
            Dimmer(v) => self.dimmer = v,
            Rotation(v) => self.rotation = v,
            FixedColor(v) => self.fixed_color = v,
            ColorRotate(v) => self.color_rotate = v,
            ColorRotation(v) => self.color_rotation = v,
        };
        Self::emit(sc, emitter);
    }
}

impl AnimatedFixture for H2O {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        _group_controls: &FixtureGroupControls,
        animation_vals: &super::animation_target::TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        let mut color_rotation = self.color_rotation.val();
        let mut dimmer = self.dimmer.val();
        let mut rotation = self.rotation.val();
        for (val, target) in animation_vals {
            use AnimationTarget::*;
            match target {
                // FIXME: might want to do something nicer for unipolar values
                ColorRotation => color_rotation += val,
                Rotation => rotation += val,
                Dimmer => dimmer += val,
            }
        }
        dmx_buf[0] = unipolar_to_range(0, 255, UnipolarFloat::new(dimmer));
        dmx_buf[1] = bipolar_to_split_range(BipolarFloat::new(rotation), 120, 10, 135, 245, 0);
        if self.color_rotate {
            dmx_buf[2] =
                bipolar_to_split_range(BipolarFloat::new(color_rotation), 186, 128, 197, 255, 187);
        } else {
            dmx_buf[2] = self.fixed_color.as_dmx();
        }
    }
}

impl ControllableFixture for H2O {
    fn control(
        &mut self,
        msg: FixtureControlMessage,
        emitter: &mut dyn crate::osc::EmitControlMessage,
    ) -> anyhow::Result<()> {
        self.handle_state_change(
            *msg.unpack_as::<ControlMessage>().context(Self::NAME)?,
            emitter,
        );
        Ok(())
    }

    fn emit_state(&self, emitter: &mut dyn crate::osc::EmitControlMessage) {
        use StateChange::*;
        Self::emit(Dimmer(self.dimmer), emitter);
        Self::emit(Rotation(self.rotation), emitter);
        Self::emit(FixedColor(self.fixed_color), emitter);
        Self::emit(ColorRotate(self.color_rotate), emitter);
        Self::emit(ColorRotation(self.color_rotation), emitter);
    }
}

#[derive(Default, Copy, Clone, Debug, PartialEq, EnumString, EnumIter, EnumDisplay)]
pub enum FixedColor {
    #[default]
    White,
    WhiteOrange,
    Orange,
    OrangeGreen,
    Green,
    GreenBlue,
    Blue,
    BlueYellow,
    Yellow,
    YellowPurple,
    Purple,
    PurpleWhite,
}

impl FixedColor {
    fn as_dmx(self) -> u8 {
        use FixedColor::*;
        match self {
            White => 0,
            WhiteOrange => 11,
            Orange => 22,
            OrangeGreen => 33,
            Green => 44,
            GreenBlue => 55,
            Blue => 66,
            BlueYellow => 77,
            Yellow => 88,
            YellowPurple => 99,
            Purple => 110,
            PurpleWhite => 121,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum StateChange {
    Dimmer(UnipolarFloat),
    Rotation(BipolarFloat),
    FixedColor(FixedColor),
    ColorRotate(bool),
    ColorRotation(BipolarFloat),
}

// H2O has no controls that are not represented as state changes.
pub type ControlMessage = StateChange;

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    EnumString,
    EnumIter,
    EnumDisplay,
    FromPrimitive,
    ToPrimitive,
)]
pub enum AnimationTarget {
    #[default]
    Dimmer,
    Rotation,
    ColorRotation,
}

impl AnimationTarget {
    /// Return true if this target is unipolar instead of bipolar.
    #[allow(unused)]
    pub fn is_unipolar(&self) -> bool {
        matches!(self, Self::Dimmer)
    }
}
