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
    pan_spin_active: Bool<()>,
    #[channel_control]
    #[animate]
    pan_spin: ChannelKnobBipolar<BipolarSplitChannelMirror>,
    #[animate]
    tilt: Bipolar<RenderBipolarToCoarseAndFine>,
    tilt_spin_active: Bool<()>,
    #[animate]
    tilt_spin: BipolarSplitChannel,
    #[channel_control]
    #[animate]
    tilt_macro_speed: ChannelKnobUnipolar<UnipolarChannel>,
    tilt_macro: TiltMacroSelect,
}

impl Default for SwizzleStick {
    fn default() -> Self {
        Self {
            pan: Bipolar::coarse_fine("Pan", 0)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            pan_spin_active: Bool::new_off("PanSpinActive", ()),
            pan_spin: Bipolar::split_channel("PanSpin", 2, 129, 255, 1, 127, 128)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(1),
            tilt: Bipolar::coarse_fine("Tilt", 0).with_detent(), // DMX buf offset will be set manually for each head
            tilt_spin_active: Bool::new_off("TiltSpinActive", ()),
            tilt_spin: Bipolar::split_channel("TiltSpin", 0, 129, 255, 1, 127, 128).with_detent(), // DMX buf offset will be set manually for each head
            tilt_macro_speed: Unipolar::full_channel("TiltMacroSpeed", 19).with_channel_knob(2),
            tilt_macro: TiltMacroSelect::default(),
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

    const PATCH_NOTES: &'static str = "Mechanical control only. \
        Set fixture to single control mode, Advanced 47-channel personality. \
        Patch one fixture for each head, setting the head index for each. \
        Create a second Color group, patch five RGBW colors starting at the fixture's address plus 27 \
        (eg if addressed at 1, start Color addresses at 28.";
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
register_touchosc_template!(SwizzleStick);

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
            if self.pan_spin_active.val() {
                self.pan_spin.render(
                    group_controls,
                    animation_vals.filter(&AnimationTarget::PanSpin),
                    dmx_buf,
                );
            } else {
                dmx_buf[2] = 0;
            }

            self.tilt_macro.render(dmx_buf); // Ch19 (buf 18): tilt macro select
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
        if self.tilt_spin_active.val() {
            self.tilt_spin.render(
                group_controls,
                animation_vals.filter(&AnimationTarget::TiltSpin),
                &mut dmx_buf[tilt_spin_buf_offset..tilt_spin_buf_offset + 1],
            );
        } else {
            dmx_buf[tilt_spin_buf_offset] = 0;
        }
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

const TILT_MACRO_SELECT_LABEL: LabelArray = LabelArray {
    control: "TiltMacroLabel",
    n: 1,
    empty_label: "",
};

/// Discrete tilt-macro selector: a unipolar fader picks one of 27 bands
/// (index 0 = "no function", 1..=26 = tilt macros), rendering the DMX value at
/// the center of the selected band and reporting the selected index as a numeric
/// read-out label. Models `freedom_fries::ProgramControl`.
#[derive(Debug, DescribeControls)]
struct TiltMacroSelect {
    select: Unipolar<()>,
    #[skip_control]
    selected: usize,
}

impl Default for TiltMacroSelect {
    fn default() -> Self {
        Self {
            select: Unipolar::new("TiltMacro", ()),
            selected: 0,
        }
    }
}

impl TiltMacroSelect {
    /// Index 0 (no function) plus 26 tilt macros.
    const COUNT: usize = 27;
    const DMX_BUF_OFFSET: usize = 18;

    /// DMX value at the center of the selected band. Index 0 renders 0 (in the
    /// 0-47 "no function" band); index `k` in 1..=26 renders `48 + (k-1)*8 + 4`,
    /// the center of macro `k`'s 8-wide band (max 252 at k=26).
    fn dmx_val(&self) -> u8 {
        if self.selected == 0 {
            0
        } else {
            (44 + self.selected * 8) as u8
        }
    }

    fn render(&self, dmx_buf: &mut [u8]) {
        dmx_buf[Self::DMX_BUF_OFFSET] = self.dmx_val();
    }

    fn emit_label(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        let label = if self.selected == 0 {
            "off".to_string()
        } else {
            self.selected.to_string()
        };
        TILT_MACRO_SELECT_LABEL.set([label].into_iter(), emitter);
    }
}

impl OscControl<()> for TiltMacroSelect {
    fn control_direct(
        &mut self,
        _val: (),
        _emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<()> {
        bail!("direct control is not implemented for TiltMacroSelect");
    }

    fn emit_state(&self, emitter: &dyn crate::osc::EmitScopedOscMessage) {
        self.select.emit_state(emitter);
        self.emit_label(emitter);
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &dyn crate::osc::EmitScopedOscMessage,
    ) -> anyhow::Result<bool> {
        if self.select.control(msg, emitter)? {
            // unipolar_to_range(0, 26, ..) is always in 0..=26; clamp anyway so
            // the fader can never index past the last band.
            self.selected = (unipolar_to_range(0, (Self::COUNT - 1) as u8, self.select.val())
                as usize)
                .min(Self::COUNT - 1);
            self.select.emit_state(emitter);
            self.emit_label(emitter);
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use rosc::{OscMessage, OscType};

    use crate::osc::{MockEmitter, OscClientId, OscControlMessage};

    use super::*;

    fn make_msg(addr: &str, arg: OscType) -> OscControlMessage {
        OscControlMessage::new(
            OscMessage {
                addr: addr.to_string(),
                args: vec![arg],
            },
            OscClientId::example(),
        )
        .unwrap()
    }

    /// Every selected index renders a DMX value inside that macro's band:
    /// index 0 -> 0 (no-function band 0-47); index k in 1..=26 -> the center of
    /// macro k's 8-wide band.
    #[test]
    fn dmx_val_lands_in_each_band() {
        let mut ctrl = TiltMacroSelect::default();
        assert_eq!(ctrl.dmx_val(), 0);
        for k in 1..TiltMacroSelect::COUNT {
            ctrl.selected = k;
            let v = ctrl.dmx_val();
            let lo = 48 + (k as u8 - 1) * 8;
            assert!(
                (lo..=lo + 7).contains(&v),
                "index {k} rendered {v}, outside band {lo}..={}",
                lo + 7
            );
        }
        // Spot-check the first two centers and the top of the range.
        for (idx, expected) in [(1usize, 52u8), (2, 60), (26, 252)] {
            ctrl.selected = idx;
            assert_eq!(ctrl.dmx_val(), expected);
        }
    }

    /// The fader maps to a band index, and the emitted read-out label is that
    /// index as a number. Boundaries: 0.0 -> off, 0.5 -> 13, 1.0 -> 26.
    #[test]
    fn fader_selects_band_and_emits_numeric_label() {
        for (arg, idx) in [(0.5, 13), (1.0, 26)] {
            let mut ctrl = TiltMacroSelect::default();
            let emitter = MockEmitter::new();
            let handled = ctrl
                .control(&make_msg("/g/TiltMacro", OscType::Float(arg)), &emitter)
                .unwrap();
            assert!(handled);
            assert_eq!(ctrl.selected, idx);
            let msgs = emitter.take();
            let label = msgs
                .iter()
                .find(|(c, _)| c == "TiltMacroLabel/0")
                .expect("a TiltMacroLabel/0 message");
            assert_eq!(label.1, OscType::String(idx.to_string()));
        }
    }

    /// Sweeping the full fader range never errors and never selects past the last
    /// band — the no-panic / fuzz-safety guarantee.
    #[test]
    fn full_sweep_never_errs() {
        let mut ctrl = TiltMacroSelect::default();
        let emitter = MockEmitter::new();
        for i in 0..=1000 {
            let handled = ctrl
                .control(
                    &make_msg("/g/TiltMacro", OscType::Float(i as f32 / 1000.0)),
                    &emitter,
                )
                .unwrap();
            assert!(handled);
            assert!(ctrl.selected < TiltMacroSelect::COUNT);
        }
    }

    /// emit_state broadcasts both the fader value and the numeric read-out label.
    #[test]
    fn emit_state_emits_fader_and_label() {
        let ctrl = TiltMacroSelect {
            selected: 5,
            ..Default::default()
        };
        let emitter = MockEmitter::new();
        ctrl.emit_state(&emitter);
        let msgs = emitter.take();
        assert!(msgs.iter().any(|(c, _)| c == "TiltMacro"));
        assert!(
            msgs.iter()
                .any(|(c, a)| c == "TiltMacroLabel/0" && *a == OscType::String("5".to_string())),
            "expected TiltMacroLabel/0 = \"5\", got {msgs:?}"
        );
    }
}
