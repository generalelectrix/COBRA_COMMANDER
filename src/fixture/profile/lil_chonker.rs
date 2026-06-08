//! "Lil Chonker" - the "Chode" 90W spot moving head (15-channel mode).
//!
//! Color wheel, two gobo wheels (one fixed, one rotating), prism, focus.
//! Color and gobo DMX values are placeholders pending a hardware mapping pass.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 15]
#[strobe(Short)]
pub struct LilChonker {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    color: LabeledSelect,
    /// Fixed (non-rotating) gobo wheel.
    gobo: IndexedSelectMult,
    /// Rotating gobo wheel selection; its spin is driven by `gobo_rotation`.
    rotating_gobo: IndexedSelectMult,
    #[channel_control]
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    /// Insert the prism. Gates `prism_rotation`, which shares its DMX channel.
    prism: Bool<()>,
    #[channel_control]
    #[animate]
    prism_rotation: ChannelKnobUnipolar<UnipolarChannel>,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
    /// Bipolar so it rests at mid-throw and rides the positioner's bipolar focus offset.
    #[animate]
    focus: BipolarChannel,
}

impl Default for LilChonker {
    fn default() -> Self {
        Self {
            // Ch 6: dimmer 0-100%. Brightness lives here; strobing modulates it.
            dimmer: Unipolar::full_channel("Dimmer", 5)
                .strobed()
                .with_channel_level(),
            // Ch 8: color wheel — open + 7 colors at steps of 20. `SplitColor` adds 10,
            // landing on the half-step between adjacent colors (e.g. 10 = open/red).
            color: LabeledSelect::new(
                "Color",
                7,
                vec![
                    ("Open", 0),
                    ("Red", 20),
                    ("Green", 40),
                    ("Blue", 60),
                    ("Yellow", 80),
                    ("Orange", 100),
                    ("Magenta", 120),
                    ("Cyan", 140),
                ],
            )
            .with_split(10),
            // Ch 12: fixed gobo wheel — open + 8 gobos (9 positions). dmx = index * 11 + 5.
            // Gobos by index: 0 open, 1 gears, 2 breakup, 3 diamonds, 4 bubbles,
            // 5 asym tri spiral, 6 basket, 7 teeth, 8 pyramid.
            gobo: IndexedSelect::multiple("Gobo", 11, false, 9, 11, 5),
            // Ch 10: rotating gobo wheel — open + 6 gobos (7 positions). dmx = index * 10 + 5.
            // Gobos by index: 0 open, 1 breakup, 2 triangle, 3 spiral,
            // 4 pents on blue (litho, white on blue), 5 three dots, 6 offset dot.
            rotating_gobo: IndexedSelect::multiple("RotatingGobo", 9, false, 7, 10, 5),
            // Ch 11: rotating-gobo spin. Forward 61-158, reverse 159-255, stop ~30.
            gobo_rotation: Bipolar::split_channel("GoboRotation", 10, 61, 158, 159, 255, 30)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            prism: Bool::new_off("Prism", ()),
            // Ch 13: shares the channel with `prism`. 0-19 = out; 20-255 = in, rotating slow→fast.
            prism_rotation: Unipolar::channel("PrismRotation", 12, 20, 255).with_channel_knob(1),
            // Ch 1/2: pan coarse + fine (adjacent).
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            // Ch 3/4: tilt coarse + fine (adjacent).
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(false),
            // Ch 9: focus, near to far. Bipolar center (0.0) = mid focus (~128).
            focus: Bipolar::channel("Focus", 8, 0, 255),
        }
    }
}

impl AnimatedFixture for LilChonker {
    type Target = AnimationTarget;

    fn positioner_axes() -> Option<crate::positioner::PositionerAxes<Self::Target>> {
        Some(crate::positioner::PositionerAxes {
            x: AnimationTarget::Pan,
            y: AnimationTarget::Tilt,
            focus: Some(AnimationTarget::Focus),
        })
    }

    fn render_with_animations<A>(
        &self,
        group_controls: &FixtureGroupControls,
        animation_vals: &A,
        dmx_buf: &mut [u8],
    ) where
        A: TargetedAnimationValues<Self::Target>,
    {
        self.pan.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        );
        dmx_buf[4] = 0; // Ch 5: X/Y movement speed, fastest (0 = fast)
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        );
        dmx_buf[6] = 0; // Ch 7: fixture strobe off — Cobra strobes via the dimmer
        self.color
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.focus.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Focus),
            dmx_buf,
        );
        self.rotating_gobo
            .render(group_controls, std::iter::empty(), dmx_buf);
        self.gobo_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        );
        self.gobo
            .render(group_controls, std::iter::empty(), dmx_buf);
        if self.prism.val() {
            self.prism_rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::PrismRotation),
                dmx_buf,
            );
        } else {
            dmx_buf[12] = 0; // Ch 13: prism retracted (out)
        }
        dmx_buf[13] = 0; // Ch 14: AUTO model — disabled
        dmx_buf[14] = 0; // Ch 15: reset — disabled
    }
}
