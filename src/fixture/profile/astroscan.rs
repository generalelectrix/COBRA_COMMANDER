//! Clay Paky Astroscan - drunken sailor extraordinaire
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control)]
pub struct Astroscan {
    lamp_on: BoolChannel,
    #[channel_control]
    #[animate]
    shutter: ChannelLevelUnipolar<DimmerStrobe>,
    #[animate]
    iris: UnipolarChannel,
    color: LabeledSelect,
    gobo: IndexedSelectMult,
    #[channel_control]
    #[animate]
    mirror_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[channel_control]
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    pan: BipolarChannelMirror,
    #[animate]
    tilt: BipolarChannelMirror,
}

impl Default for Astroscan {
    fn default() -> Self {
        Self {
            lamp_on: Bool::full_channel("LampOn", 2),
            shutter: ShutterStrobe::new(
                Unipolar::channel("Dimmer", 3, 0, 139),
                Strobe::channel("Strobe", 3, 140, 243, 0),
            )
            .with_channel_level(),
            iris: Unipolar::full_channel("Iris", 0),
            color: LabeledSelect::new(
                "Color",
                1,
                vec![
                    ("Open", 0),
                    ("Red", 14),
                    ("Yellow", 32),
                    ("Violet", 51),
                    ("Green", 67),
                    ("Orange", 81),
                    ("Blue", 98),
                    ("Pink", 115), // 127 back to white
                ],
            ),
            gobo: IndexedSelect::multiple("Gobo", 6, false, 5, 55, 0),
            gobo_rotation: Bipolar::split_channel("GoboRotation", 7, 189, 128, 193, 255, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            mirror_rotation: Bipolar::split_channel("MirrorRotation", 8, 189, 128, 193, 255, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            pan: Bipolar::channel("Pan", 4, 0, 255)
                .with_detent()
                .with_mirroring(true),
            tilt: Bipolar::channel("Tilt", 5, 0, 255)
                .with_detent()
                .with_mirroring(false),
        }
    }
}

impl PatchAnimatedFixture for Astroscan {
    const NAME: FixtureType = FixtureType("Astroscan");
    fn channel_count(&self) -> usize {
        9
    }
}

impl ControllableFixture for Astroscan {}

impl AnimatedFixture for Astroscan {
    type Target = AnimationTarget;

    fn render_with_animations(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: TargetedAnimationValues<Self::Target>,
        dmx_buf: &mut [u8],
    ) {
        self.iris
            .render(animation_vals.filter(&AnimationTarget::Iris), dmx_buf);
        self.color.render_no_anim(dmx_buf);
        self.lamp_on.render_no_anim(dmx_buf);
        self.shutter.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Shutter),
            dmx_buf,
        );
        self.pan.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );
        self.tilt.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        );
        self.gobo.render_no_anim(dmx_buf);
        self.gobo_rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        );
        self.mirror_rotation.render_with_group(
            group_controls,
            animation_vals.filter(&AnimationTarget::MirrorRotation),
            dmx_buf,
        );
    }
}
