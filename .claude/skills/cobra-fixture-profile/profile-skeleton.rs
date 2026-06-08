// SKELETON — copy to `src/fixture/profile/<name>.rs`, rename, and adapt.
// This is a moving-head spot template (pan/tilt / color wheel / two gobo wheels / prism /
// focus). Remove the parts a simpler fixture lacks. See control-reference.md.
//
// Remember to add `pub mod <name>;` to `src/fixture/profile/mod.rs`.
//
// NOTE on docstrings: this repo enforces timeless single-entity doc comments (`///`) — no
// "see other field", no process notes. Keep cross-references in plain `//` comments like these.
use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control, DescribeControls, Update, PatchFixture)]
#[channel_count = 15] // total DMX channels; every offset below must be < this
#[strobe(Short)] // Short = crisp LED flash; Long = slower fixtures
pub struct ExampleSpot {
    #[channel_control] // hardware level fader
    #[animate]
    dimmer: ChannelLevelUnipolar<UnipolarChannel>,
    color: LabeledSelect,
    /// Fixed (non-rotating) gobo wheel.
    gobo: IndexedSelectMult,
    /// Rotating gobo wheel.
    // Spin is driven by `gobo_rotation`.
    rotating_gobo: IndexedSelectMult,
    #[channel_control] // hardware knob
    #[animate]
    gobo_rotation: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    /// Prism insert.
    // Gates `prism_rotation`, which renders to the same DMX channel.
    prism: Bool<()>,
    #[channel_control]
    #[animate]
    prism_rotation: ChannelKnobUnipolar<UnipolarChannel>,
    #[animate]
    pan: Mirrored<RenderBipolarToCoarseAndFine>,
    #[animate]
    tilt: Mirrored<RenderBipolarToCoarseAndFine>,
    /// Beam focus.
    // Bipolar so it rests mid-throw and rides the positioner's bipolar focus offset.
    #[animate]
    focus: BipolarChannel,
}

impl Default for ExampleSpot {
    fn default() -> Self {
        Self {
            // Comment each line with the manual's channel number + meaning.
            dimmer: Unipolar::full_channel("Dimmer", 5)
                .strobed()
                .with_channel_level(),
            color: LabeledSelect::new(
                "Color",
                7,
                vec![("Open", 0), ("Red", 20), ("Green", 40) /* … */],
            )
            .with_split(10), // omit if the wheel has no split positions
            gobo: IndexedSelect::multiple("Gobo", 11, false, 9, 11, 5),
            rotating_gobo: IndexedSelect::multiple("RotatingGobo", 9, false, 7, 10, 5),
            gobo_rotation: Bipolar::split_channel("GoboRotation", 10, 61, 158, 159, 255, 30)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            prism: Bool::new_off("Prism", ()),
            prism_rotation: Unipolar::channel("PrismRotation", 12, 20, 255).with_channel_knob(1),
            pan: Bipolar::coarse_fine("Pan", 0).with_mirroring(true),
            tilt: Bipolar::coarse_fine("Tilt", 2).with_mirroring(false),
            focus: Bipolar::channel("Focus", 8, 0, 255),
        }
    }
}

impl AnimatedFixture for ExampleSpot {
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
        // `#[animate]` fields take a filtered iterator; selects take std::iter::empty().
        self.pan
            .render(group_controls, animation_vals.filter(&AnimationTarget::Pan), dmx_buf);
        self.tilt
            .render(group_controls, animation_vals.filter(&AnimationTarget::Tilt), dmx_buf);
        dmx_buf[4] = 0; // XY speed: fastest
        self.dimmer
            .render(group_controls, animation_vals.filter(&AnimationTarget::Dimmer), dmx_buf);
        dmx_buf[6] = 0; // fixture strobe off — Cobra strobes via the dimmer
        self.color.render(group_controls, std::iter::empty(), dmx_buf);
        self.focus
            .render(group_controls, animation_vals.filter(&AnimationTarget::Focus), dmx_buf);
        self.rotating_gobo.render(group_controls, std::iter::empty(), dmx_buf);
        self.gobo_rotation.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::GoboRotation),
            dmx_buf,
        );
        self.gobo.render(group_controls, std::iter::empty(), dmx_buf);
        // Shared-channel gate: the prism Bool gates prism_rotation on the same channel.
        if self.prism.val() {
            self.prism_rotation.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::PrismRotation),
                dmx_buf,
            );
        } else {
            dmx_buf[12] = 0; // prism out
        }
        dmx_buf[13] = 0; // AUTO model — disabled
        dmx_buf[14] = 0; // reset — disabled
        // CHECK: every channel 0..channel_count is written exactly once (no clobbering).
    }
}
