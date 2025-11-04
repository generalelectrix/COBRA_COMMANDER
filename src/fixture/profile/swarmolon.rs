//! I said I'd never fucking do it again but here's a profile for the swarmolon.
//! This is designed to be patched as three "independent" fixtures, so that
//! control is spread across three upfaders for the use case of deployment with
//! nothing but a fader wing.

use std::{collections::HashSet, sync::LazyLock};

use crate::fixture::{fixture::FixtureType, patch::PatchFixture};

mod derby {
    use crate::fixture::prelude::*;

    #[derive(Debug, PatchFixture, Control, Update, EmitState)]
    #[channel_count = 9]
    #[strobe(Short)]
    pub struct SwarmolonDerby {
        #[channel_control]
        shutter: ChannelLevelBool<Bool<()>>,
        #[channel_control]
        color: ChannelKnobUnipolar<UnipolarChannel>,
        #[channel_control]
        #[animate]
        rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    }

    impl Default for SwarmolonDerby {
        fn default() -> Self {
            Self {
                shutter: Bool::new_off("Shutter", ()).with_channel_level(),
                color: UnipolarChannel::channel("Color", 1, 10, 164).with_channel_knob(0),
                rotation: Bipolar::split_channel("R", 7, 5, 127, 134, 255, 0)
                    .with_detent()
                    .with_mirroring(true)
                    .with_channel_knob(1),
            }
        }
    }

    impl AnimatedFixture for SwarmolonDerby {
        type Target = AnimationTarget;
        fn render_with_animations(
            &self,
            group_controls: &FixtureGroupControls,
            animation_vals: &TargetedAnimationValues<Self::Target>,
            dmx_buf: &mut [u8],
        ) {
            dmx_buf[0] = 0; // automatic programs
            let shutter_open = group_controls
                .strobe_shutter()
                .unwrap_or(self.shutter.control.val());
            if shutter_open {
                self.color
                    .render(group_controls, std::iter::empty(), dmx_buf);
            } else {
                dmx_buf[1] = 0;
            }
            dmx_buf[2] = 0; // automatic speed
            dmx_buf[3] = 0; // internal derby strobe
            self.rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::Rotation),
                dmx_buf,
            );
        }
    }
}

mod strobe {
    use crate::fixture::prelude::*;

    #[derive(Debug, PatchFixture, Control, Update, EmitState)]
    #[channel_count = 9]
    #[strobe(Short)]
    pub struct SwarmolonStrobe {
        #[channel_control]
        pattern_select: ChannelKnobUnipolar<Unipolar<()>>,
        #[channel_control]
        rate: ChannelKnobUnipolar<Unipolar<()>>,
    }

    const BUF_OFFSET: usize = 4;

    impl Default for SwarmolonStrobe {
        fn default() -> Self {
            Self {
                pattern_select: Unipolar::new("Pattern", ()).with_channel_knob(0),
                rate: Unipolar::new("Rate", ()).with_channel_knob(1),
            }
        }
    }

    impl NonAnimatedFixture for SwarmolonStrobe {
        fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
            let strobe_on =
                group_controls.strobe_enabled && group_controls.strobe_clock().strobe_on();
            if !strobe_on {
                dmx_buf[BUF_OFFSET] = 0;
                return;
            }
            // compute pattern select index and offset
            let pattern = unipolar_to_range(1, 10, self.pattern_select.control.val());
            let rate = unipolar_to_range(9, 0, self.rate.control.val());

            dmx_buf[BUF_OFFSET] = rate + pattern * 10;
        }
    }
}

mod lasers {
    use crate::fixture::prelude::*;

    #[derive(Debug, PatchFixture, Control, Update, EmitState)]
    #[channel_count = 9]
    #[strobe(Short)]
    pub struct SwarmolonLasers {
        #[channel_control]
        shutter: ChannelLevelBool<Bool<()>>,
        #[channel_control]
        color: ChannelKnobUnipolar<Unipolar<()>>,
        #[channel_control]
        #[animate]
        rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    }

    impl Default for SwarmolonLasers {
        fn default() -> Self {
            Self {
                shutter: Bool::new_off("Shutter", ()).with_channel_level(),
                color: Unipolar::new("Color", ()).with_channel_knob(0),
                rotation: Bipolar::split_channel("R", 8, 5, 127, 134, 255, 0)
                    .with_detent()
                    .with_mirroring(true)
                    .with_channel_knob(1),
            }
        }
    }

    impl AnimatedFixture for SwarmolonLasers {
        type Target = AnimationTarget;
        fn render_with_animations(
            &self,
            group_controls: &FixtureGroupControls,
            animation_vals: &TargetedAnimationValues<Self::Target>,
            dmx_buf: &mut [u8],
        ) {
            let shutter_open = group_controls
                .strobe_shutter()
                .unwrap_or(self.shutter.control.val());
            dmx_buf[5] = if shutter_open {
                let knob = self.color.control.val().val();
                if knob < (1. / 3.) {
                    10
                } else if knob < (2. / 3.) {
                    50
                } else {
                    255
                }
            } else {
                0
            };
            self.rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::Rotation),
                dmx_buf,
            );
        }
    }
}

static AFFINITY: LazyLock<HashSet<FixtureType>> = LazyLock::new(|| {
    [
        derby::SwarmolonDerby::NAME,
        strobe::SwarmolonStrobe::NAME,
        lasers::SwarmolonLasers::NAME,
    ]
    .into_iter()
    .collect()
});

/// Return the set of fixture types that have patch affinity (aka we allow them)
/// to be patched over each other.
pub fn affinity() -> &'static HashSet<FixtureType> {
    &AFFINITY
}
