//! Martin MAC 700 Profile — 16-bit Extended mode (31 DMX channels).
//!
//! An arc-lamp moving-head spot: subtractive CMY color mixing (computed from an
//! embedded HSLuv color and rendered to the 16-bit dimmer and CMY channels), a
//! dichroic color wheel,
//! a rotating gobo wheel plus its spin, a static gobo wheel, a prism, iris,
//! focus, zoom, and 16-bit pan/tilt. Each color reproduces its HSLuv value
//! faithfully as `dimmer × CMY` — chromaticity in the flags, brightness on the
//! 16-bit dimmer. The mechanical flags cannot slew at strobe rate, so a strobe
//! flashes only the dimmer and holds the flags, keeping hue and saturation steady.
//!
//! The onboard strobe, macros, animation wheel, and pan/tilt/effects speed
//! channels are pinned to safe values — Cobra strobes globally via the dimmer,
//! and macro/auto behavior is antithetical to live control.
use crate::color::{AnalyticalCmy, ColorSpace};
use crate::fixture::{color::Color, prelude::*};

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 31]
#[strobe(Long)]
pub struct Mac700 {
    // Ch2-9: CMY color mixing + the 16-bit dimmer, both rendered from this HSLuv
    // color. Declared first so it claims hardware knobs 0=Hue, 1=Sat,
    // 2=LightnessBoost (and the level fader) before any other control.
    #[channel_control]
    #[animate_subtarget(Hue, Sat, Val)]
    color: Color,

    // Ch10: dichroic color wheel. SplitColor parks between adjacent slots.
    color_wheel: LabeledSelect,

    // Ch12: rotating gobo wheel selection (continuous-rotation band).
    rotating_gobo: LabeledSelect,

    // Ch13: rotating gobo spin.
    #[channel_control]
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,

    // Ch15: static gobo wheel.
    static_gobo: LabeledSelect,

    // Ch19: prism. Gates `prism_rotation`, which shares its DMX channel.
    prism: Bool<()>,
    #[channel_control]
    #[animate]
    prism_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,

    // Ch20: iris (8-bit; the pulse-effect band is dropped).
    #[channel_control]
    #[animate]
    iris: ChannelKnobUnipolar<UnipolarChannel>,

    // Ch22/23: focus, 16-bit. Bipolar so it rests mid-throw and rides the
    // positioner focus offset.
    #[animate]
    focus: Bipolar<RenderBipolarToCoarseAndFine>,

    // Ch24/25: zoom, 16-bit.
    #[channel_control]
    #[animate]
    zoom: ChannelKnobUnipolar<Unipolar<RenderUnipolarToCoarseAndFine>>,

    // Ch26/27 & 28/29: 16-bit pan/tilt.
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
}

impl Default for Mac700 {
    fn default() -> Self {
        Self {
            color: Color::for_subcontrol(None, ColorSpace::Hsluv),
            color_wheel: LabeledSelect::new(
                "Color",
                9,
                vec![
                    ("Open", 0),
                    ("Blue", 17),
                    ("Green", 34),
                    ("Pink", 51),
                    ("Orange", 68),
                    ("HalfMinusGreen", 85),
                    ("CTC3200", 102),
                    ("CTC5500", 119),
                    ("Red", 136),
                ],
            )
            .with_split(8),
            rotating_gobo: LabeledSelect::new(
                "RotatingGobo",
                11,
                vec![
                    ("Open", 0),
                    ("Spiral", 37),
                    ("RadialCircles", 41),
                    ("FusedDichro", 45),
                    ("MilkyWay", 49),
                    ("Water", 53),
                    ("Flames", 57),
                ],
            ),
            // Ch13 continuous rotation: CW 3-127 (slow→fast), CCW 128-252
            // (fast→slow), stop at 0-2/253-255.
            gobo_rotation: Bipolar::split_channel("GoboRotation", 12, 3, 127, 252, 128, 1)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(3),
            static_gobo: LabeledSelect::new(
                "StaticGobo",
                14,
                vec![
                    ("Open", 0),
                    ("Crackle", 11),
                    ("TrianglesSmall", 22),
                    ("TyeDye", 33),
                    ("Globo", 44),
                    ("Worms", 55),
                    ("Bio", 66),
                    ("LeafBreakup", 77),
                    ("WhirlPool", 88),
                    ("TwoTone", 99),
                ],
            ),
            prism: Bool::new_off("Prism", ()),
            // Ch19: off 0-19; CW 90-149 (slow→fast), CCW 20-79 (fast→slow),
            // stop ~85. Gated out (0) when the prism is retracted.
            prism_rotation: Bipolar::split_channel("PrismRotation", 18, 90, 149, 79, 20, 85)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(4),
            // Ch20: 0-199 open→closed; the 200-255 pulse band is dropped.
            iris: Unipolar::channel("Iris", 19, 0, 199).with_channel_knob(5),
            focus: Bipolar::coarse_fine("Focus", 21),
            zoom: Unipolar::coarse_fine("Zoom", 23).with_channel_knob(6),
            pan: Bipolar::coarse_fine("Pan", 25)
                .with_detent()
                .with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 27)
                .with_detent()
                .with_mirroring(false),
        }
    }
}

impl AnimatedFixture for Mac700 {
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
        dmx_buf[0] = 30; // Ch1: shutter open (lamp strike/reset via fixture menu)

        // Ch2/3 dimmer + Ch4-9 CMY, from the embedded HSLuv color. Each of the
        // four drives renders to its own 16-bit coarse/fine channel pair; their
        // adjacency here is incidental.
        let cmy =
            self.color
                .cmy_dimmer(&AnalyticalCmy, group_controls, &animation_vals.subtarget());
        RenderUnipolarToCoarseAndFine { dmx_buf_offset: 1 }.render(&cmy.dimmer, dmx_buf);
        RenderUnipolarToCoarseAndFine { dmx_buf_offset: 3 }.render(&cmy.cyan, dmx_buf);
        RenderUnipolarToCoarseAndFine { dmx_buf_offset: 5 }.render(&cmy.magenta, dmx_buf);
        RenderUnipolarToCoarseAndFine { dmx_buf_offset: 7 }.render(&cmy.yellow, dmx_buf);

        self.color_wheel
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch10
        dmx_buf[10] = 0; // Ch11: color wheel fine — discrete selection

        self.rotating_gobo
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch12
        self.gobo_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        ); // Ch13
        dmx_buf[13] = 0; // Ch14: rotating gobo fine indexing — continuous mode

        self.static_gobo
            .render(group_controls, std::iter::empty(), dmx_buf); // Ch15

        dmx_buf[15] = 0; // Ch16: static/color macros, random CMY — disabled
        dmx_buf[16] = 0; // Ch17: animation wheel position — disabled
        dmx_buf[17] = 0; // Ch18: animation wheel index/rotation — disabled

        if self.prism.val() {
            self.prism_rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::PrismRotation),
                dmx_buf,
            ); // Ch19
        } else {
            dmx_buf[18] = 0; // prism retracted (out)
        }

        self.iris.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Iris),
            dmx_buf,
        ); // Ch20
        dmx_buf[20] = 0; // Ch21: iris fine — 8-bit control

        self.focus.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Focus),
            dmx_buf,
        ); // Ch22/23
        self.zoom.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Zoom),
            dmx_buf,
        ); // Ch24/25
        self.pan.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        ); // Ch26/27
        self.tilt.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Tilt),
            dmx_buf,
        ); // Ch28/29

        dmx_buf[29] = 0; // Ch30: pan/tilt speed — tracking (fast)
        dmx_buf[30] = 0; // Ch31: effects speed — tracking
    }
}
