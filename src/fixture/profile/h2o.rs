//! Intuitive control profile for the American DJ H2O DMX Pro.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, PatchAnimatedFixture)]
#[channel_count = 3]
pub struct H2O {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    fixed_color: LabeledSelect,
    color_rotate: Bool<()>,
    #[channel_control]
    #[animate]
    color_rotation: ChannelKnobBipolar<BipolarSplitChannel>,
}

impl Default for H2O {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 0).with_channel_level(),
            rotation: Bipolar::split_channel("Rotation", 1, 120, 10, 135, 245, 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            fixed_color: LabeledSelect::new(
                "FixedColor",
                2,
                vec![
                    ("White", 0),
                    ("WhiteOrange", 11),
                    ("Orange", 22),
                    ("OrangeGreen", 33),
                    ("Green", 44),
                    ("GreenBlue", 55),
                    ("Blue", 66),
                    ("BlueYellow", 77),
                    ("Yellow", 88),
                    ("YellowPurple", 99),
                    ("Purple", 110),
                    ("PurpleWhite", 121),
                ],
            ),
            color_rotate: Bool::new_off("ColorRotate", ()),
            color_rotation: Bipolar::split_channel("ColorRotation", 2, 186, 128, 197, 255, 187)
                .with_detent()
                .with_channel_knob(1),
        }
    }
}

impl AnimatedFixture for H2O {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.dimmer.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        self.rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        if self.color_rotate.val() {
            self.color_rotation.render_with_group(
                group_controls,
                animation_vals.filter(&AnimationTarget::ColorRotation),
                dmx_buf,
            );
        } else {
            self.fixed_color.render_no_anim(dmx_buf);
        }
    }
}

impl ControllableFixture for H2O {}
