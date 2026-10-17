//! SHEHDS Galaxyjet 300W 3-in-1 beam moving head with LED ring — 23-channel mode.
//!
//! A 300W LED beam/spot: color wheel, a fixed gobo wheel, a rotating gobo
//! wheel plus its spin, a 5-facet prism plus its spin, frost, zoom, 16-bit
//! focus, 16-bit dimmer, and 16-bit pan/tilt.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 23]
#[strobe(Short)]
#[patch_notes = "Set fixture to 23CH mode (menu: Address > Channel > 23CHB)."]
pub struct Galaxyjet {
    #[channel_control]
    #[animate]
    dimmer: ChannelLevelUnipolar<Unipolar<RenderUnipolarToCoarseAndFine>>,
    color: LabeledSelect,
    /// Fixed (non-rotating) gobo wheel.
    gobo: IndexedSelectMult,
    /// Rotating gobo wheel.
    // Spin is driven by `gobo_rotation`.
    rotating_gobo: IndexedSelectMult,
    #[channel_control]
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    /// Prism insert.
    prism: BoolChannel,
    #[channel_control]
    #[animate]
    prism_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    /// Frost filter insert.
    frost: BoolChannel,
    #[animate]
    zoom: ChannelKnobUnipolar<UnipolarChannel>,
    /// Beam focus.
    // Bipolar so it rests mid-throw and rides the positioner's bipolar focus offset.
    #[animate]
    focus: Bipolar<RenderBipolarToCoarseAndFine>,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
}

impl Default for Galaxyjet {
    fn default() -> Self {
        Self {
            // Ch7/8: dimmer, 16-bit. Brightness lives here; strobing modulates it.
            dimmer: Unipolar::coarse_fine("Dimmer", 6)
                .strobed()
                .with_channel_level(),
            // Ch9: color wheel — open + 9 colors. PLACEHOLDER names and values: the
            // manual gives no slot list. Map on the head.
            color: LabeledSelect::new(
                "Color",
                8,
                vec![
                    ("Open", 0),
                    ("Color1", 20),
                    ("Color2", 40),
                    ("Color3", 60),
                    ("Color4", 80),
                    ("Color5", 100),
                    ("Color6", 120),
                    ("Color7", 140),
                    ("Color8", 160),
                    ("Color9", 180),
                ],
            ),
            // Ch11: fixed gobo wheel — open + 5 gobos + flame (7 positions).
            // dmx = index * 10 + 5. PLACEHOLDER spacing: the manual gives no slot list.
            gobo: IndexedSelect::multiple("Gobo", 10, false, 7, 10, 5),
            // Ch12: rotating gobo wheel — open + 7 gobos (8 positions).
            // dmx = index * 10 + 5. PLACEHOLDER spacing: the manual gives no slot list.
            rotating_gobo: IndexedSelect::multiple("RotatingGobo", 11, false, 8, 10, 5),
            // Ch13: rotating-gobo spin. CW 193-255 (slow→fast), CCW 190-128
            // (slow→fast), stop 191-192. The 0-127 index band is dropped.
            gobo_rotation: Bipolar::split_channel("GoboRotation", 12, 193, 255, 190, 128, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            // Ch14: prism. 0-127 out, 128-255 in.
            prism: Bool::full_channel("Prism", 13),
            // Ch15: prism spin. Per the manual, CW 128-190 (slow→fast), CCW 255-193
            // (slow→fast), stop 191-192 — fast ends abut the stop, the reverse of
            // the gobo spin. The 0-127 index band is dropped.
            prism_rotation: Bipolar::split_channel("PrismRotation", 14, 128, 190, 255, 193, 191)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            // Ch16: frost. 0-127 off, 128-255 on.
            frost: Bool::full_channel("Frost", 15),
            // Ch17: zoom, linear.
            zoom: Unipolar::full_channel("Zoom", 16).with_channel_knob(2),
            // Ch18/19: focus, 16-bit, linear. Bipolar center (0.0) = mid focus.
            focus: Bipolar::coarse_fine("Focus", 17),
            // Ch1/2: pan coarse + fine.
            pan: Bipolar::coarse_fine("Pan", 0)
                .with_detent()
                .with_mirroring(true),
            // Ch3/4: tilt coarse + fine.
            tilt: Bipolar::coarse_fine("Tilt", 2)
                .with_detent()
                .with_mirroring(false),
        }
    }
}

impl AnimatedFixture for Galaxyjet {
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
        ); // Ch1/2
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        ); // Ch3/4
        dmx_buf[4] = 0; // Ch5: pan/tilt speed — fastest
        dmx_buf[5] = 255; // Ch6: fixture strobe open — Cobra strobes via the dimmer
        self.dimmer.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Dimmer),
            dmx_buf,
        ); // Ch7/8
        self.color
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch9
        dmx_buf[9] = 0; // Ch10: color wheel fine — discrete selection
        self.gobo
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch11
        self.rotating_gobo
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch12
        self.gobo_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        ); // Ch13
        self.prism
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch14
        self.prism_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::PrismRotation),
            dmx_buf,
        ); // Ch15
        self.frost
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch16
        self.zoom.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Zoom),
            dmx_buf,
        ); // Ch17
        self.focus.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Focus),
            dmx_buf,
        ); // Ch18/19
        dmx_buf[19] = 0; // Ch20: reset — disabled
        dmx_buf[20] = 0; // Ch21: LED ring strobe — off
        dmx_buf[21] = 0; // Ch22: LED ring effect mode — off
        dmx_buf[22] = 0; // Ch23: LED ring effect speed
    }
}
