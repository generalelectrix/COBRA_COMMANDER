//! Chauvet Rogue R1 FX-B continuous-pan 5-head continuous-tilt RGBW LED effect.
//! Aka the "swizzle stick". This is a strange beast, and patching it in Cobra
//! Commander is a bit awkward.
//!
//! Use 47 channel "Advanced" mode to make it compatible with this patching.
//!
//! We represent the head's mechanical motion as
//! one group, defined by this profile, which itself has two patch modes for
//! patching the "master" head vs the subsequent four heads. The order of the
//! heads in the group can be manipulated to create forced symmetry in the patch.
//! The master head controls the overall pan and also sets the expected values
//! for the rest of the patch to work correctly. This requires several fixtures
//! in the group to patch over each other, which is tedious and error-prone
//! but at the moment its the best we can do. We do this via the "patch affinity"
//! hack. PATCH WITH CAUTION!
use log::error;
use strum_macros::{Display, EnumIter, VariantArray};

use crate::fixture::prelude::*;

#[derive(Debug, Control, DescribeControls, EmitState, Update)]
pub struct SwizzleStick {
    #[channel_control]
    #[animate]
    pan: ChannelKnobBipolar<Mirrored<RenderBipolarToCoarseAndFine>>,
    #[channel_control]
    #[animate]
    pan_spin: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    tilt: Bipolar<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt_spin: BipolarSplitChannel,
    #[channel_control]
    #[animate]
    tilt_macro_speed: ChannelKnobUnipolar<UnipolarChannel>,
}

impl Default for SwizzleStick {
    fn default() -> Self {
        Self {
            pan: Bipolar::coarse_fine("Pan", 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            pan_spin: Bipolar::split_channel("PanSpin", 2, 129, 255, 1, 127, 128)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            tilt: Bipolar::coarse_fine("Tilt", 0).with_detent(), // DMX buf offset will be set manually for each head
            tilt_spin: Bipolar::split_channel("TiltSpin", 0, 129, 255, 1, 127, 128).with_detent(), // DMX buf offset will be set manually for each head
            tilt_macro_speed: Unipolar::full_channel("TiltMacroSpeed", 19).with_channel_knob(2),
        }
    }
}

#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct PatchOptions {
    #[serde(default)]
    pub head_index: HeadIndex,
}

impl PatchFixture for SwizzleStick {
    const NAME: FixtureType = FixtureType("SwizzleStick");
    type GroupOptions = NoOptions;
    type PatchOptions = PatchOptions;

    fn new(_: Self::GroupOptions) -> Self {
        Self::default()
    }
    fn can_strobe() -> Option<StrobeResponse> {
        Some(StrobeResponse::Short)
    }
    fn new_patch(_: Self::GroupOptions, options: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: 27,
            render_mode: Some(options.head_index.render_mode()),
        }
    }
}

register_patcher!(SwizzleStick);

impl AnimatedFixture for SwizzleStick {
    type Target = AnimationTarget;
    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>,
    {
        let head = match HeadIndex::model_for_mode(group_controls.render_mode) {
            Ok(m) => m,
            Err(err) => {
                error!("failed to render SwizzleStick: {err}");
                return;
            }
        };
        // Render the pan/aux controls.
        if head == HeadIndex::One {
            self.pan.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::Pan),
                dmx_buf,
            );
            self.pan_spin.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::PanSpin),
                dmx_buf,
            );
            dmx_buf[18] = 0; // TODO tilt macro select
            self.tilt_macro_speed.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::TiltMacroSpeed),
                dmx_buf,
            );
            dmx_buf[20] = 0; // meta-control - set to no function
            dmx_buf[21] = 255; // dimmer to full
            dmx_buf[22] = 20; // shutter open
            dmx_buf[23] = 0; // color macro off
            dmx_buf[24] = 0; // all heads set to on
            dmx_buf[25] = 0; // LED macro/auto program off
            dmx_buf[26] = 0; // LED macro speed
        }
        let index = head.index();
        let tilt_buf_offset = 3 + (2 * index);
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            &mut dmx_buf[tilt_buf_offset..tilt_buf_offset + 2],
        );
        let tilt_spin_buf_offset = 13 + index;
        self.tilt_spin.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::TiltSpin),
            &mut dmx_buf[tilt_spin_buf_offset..tilt_spin_buf_offset + 1],
        );
    }
}

/// Which head of the fixture does this specific patch control?
#[derive(
    Debug, Clone, Copy, Default, Eq, PartialEq, Deserialize, VariantArray, Display, EnumIter,
)]
pub enum HeadIndex {
    #[default]
    One,
    Two,
    Three,
    Four,
    Five,
}

impl EnumRenderModel for HeadIndex {}

impl HeadIndex {
    pub fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
            Self::Three => 2,
            Self::Four => 3,
            Self::Five => 4,
        }
    }
}
