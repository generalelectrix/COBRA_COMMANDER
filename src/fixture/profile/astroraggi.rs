//! Control profile for the Clay Paky Astroraggi Power.
//!
//! Two channels: continuous dome rotation and a clamshell shutter.
//!
//! The rotation channel maps only the continuous-spin zone (50%-100%): from
//! DMX 128 (max anti-clockwise) it slows to a stop at DMX 189-194, then
//! accelerates clockwise to DMX 255. The static-positioning zone below 50% is
//! left to macro programs. The shutter maps the dimmer's open range (0%-50%,
//! DMX 0-128) and strobes via the software strobe clock rather than the
//! fixture's onboard strobe, so it stays synchronized with the rest of the rig.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 2]
#[strobe(Long)]
pub struct Astroraggi {
    #[channel_control]
    #[animate]
    rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[channel_control]
    #[animate]
    shutter: ChannelLevelUnipolar<UnipolarChannel>,
}

impl Default for Astroraggi {
    fn default() -> Self {
        Self {
            rotation: Bipolar::split_channel("Rotation", 0, 194, 255, 189, 128, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            shutter: Unipolar::channel("Shutter", 1, 0, 128)
                .strobed()
                .with_channel_level(),
        }
    }
}

impl AnimatedFixture for Astroraggi {
    type Target = AnimationTarget;

    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>,
    {
        self.rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Rotation),
            dmx_buf,
        );
        self.shutter.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Shutter),
            dmx_buf,
        );
    }
}
