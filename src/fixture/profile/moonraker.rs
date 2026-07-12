//! Moonraker: 6-beam laser-fan moving head (27-channel mode).
//!
//! A yoke head that fans six independently-colored laser beams around a
//! roll-rotating central axis. Pan and tilt aim the head; a Z-rotation channel
//! rolls the fan (indexed park angle or continuous single-direction spin); six
//! RGB beams are each set to one of eight fixed colors, optionally linked to
//! beam 1. A single master channel gates every beam and carries the global
//! strobe flash.
//!
//! Tilt is safety-clamped: the head can dip beams below the horizon, so the
//! tilt throw is dynamically remapped to keep the lowest beam at or above a
//! configurable elevation floor. See the calibration block and `rescaled_tilt`.
//!
//! Motor speed, built-in strobe, and the FX channels are pinned to safe values —
//! Cobra strobes globally via the master, and macro/FX behavior is antithetical
//! to live control.
use crate::fixture::prelude::*;

// ---------------------------------------------------------------------------
// Safety-guardrail calibration. ALL PLACEHOLDERS — MEASURE ON HARDWARE.
// The geometry stays hardcoded; only the tilt-range scale is a group option.
// ---------------------------------------------------------------------------

/// Half opening angle of the 6-beam fan about the head's central axis, degrees.
/// MEASURE ON HARDWARE.
const FAN_HALF_ANGLE_DEG: f64 = 40.0;

/// Tilt geometry — base-mounted ("sitting on its base"), tilt swinging +/-90deg
/// from vertical (180deg total). These endpoints are fixed by that known range,
/// not measured: DMX ~128 (bipolar tilt = 0) points STRAIGHT UP (elevation
/// 90deg); DMX 0 and 255 (tilt = -1 / +1) point at the HORIZON (elevation 0deg),
/// 90deg to either side. Central-axis elevation is then a symmetric tent in
/// |tilt|, highest at center. (Mounting/level slop is absorbed by
/// `TILT_FLOOR_MARGIN_DEG`, not modeled here.)
///
/// `ELEV_AT_CENTER_DEG` = elevation at tilt 0 (straight up).
/// `ELEV_AT_EXTREME_DEG` = elevation at |tilt| = 1 (either horizon rail).
const ELEV_AT_CENTER_DEG: f64 = 90.0;
const ELEV_AT_EXTREME_DEG: f64 = 0.0;

/// Elevation swept between straight-up and a horizon rail.
const ELEV_SPAN_DEG: f64 = ELEV_AT_CENTER_DEG - ELEV_AT_EXTREME_DEG;

/// Head roll (fan-plane) angle, degrees, as a linear function of the `Roll`
/// fader (0..1): phi = PHASE + roll * SPAN. Pick PHASE so the fan is horizontal
/// (|sin phi| = 0) at the fader position where the beams lie flat.
/// MEASURE ON HARDWARE.
const ROLL_PHASE_DEG: f64 = 0.0;
const ROLL_SPAN_DEG: f64 = 180.0;

/// Extra headroom added to the required axis elevation, degrees, for physical
/// pointing slop and rounding. The fan geometry itself is exact, so this is not
/// load-bearing for the model. MEASURE / TUNE ON HARDWARE.
const TILT_FLOOR_MARGIN_DEG: f64 = 0.0;

/// Index -> (R, G, B) on/off triple for the 8 fixed beam colors.
/// {Off, R, G, B, C, M, Y, W}. Each byte renders as 0 (off) or 255 (on).
const BEAM_RGB: [[u8; 3]; 8] = [
    [0, 0, 0],       // 0 Off
    [255, 0, 0],     // 1 Red
    [0, 255, 0],     // 2 Green
    [0, 0, 255],     // 3 Blue
    [0, 255, 255],   // 4 Cyan
    [255, 0, 255],   // 5 Magenta
    [255, 255, 0],   // 6 Yellow
    [255, 255, 255], // 7 White
];

// ---------------------------------------------------------------------------
// Pure, unit-testable safety helpers. No panics.
// ---------------------------------------------------------------------------

/// Central-axis elevation (deg) for a hardware tilt value — the symmetric tent:
/// max at center (|t| = 0, straight up), min at either rail (|t| = 1, horizon).
///
/// The forward map, paired with the inverse `max_tilt_magnitude` that render
/// uses; exercised by the safety tests to verify the geometry end to end.
#[cfg_attr(not(test), expect(dead_code))]
fn tilt_to_elevation_deg(t: BipolarFloat) -> f64 {
    let a = t.val().abs(); // 0 at center, 1 at either horizon rail
    ELEV_AT_CENTER_DEG - a * ELEV_SPAN_DEG
}

/// Largest tilt magnitude |t| in [0, 1] whose central-axis elevation still meets
/// `e_min_central_deg`. The tent is symmetric, so the safe hardware range is
/// [-max, +max] about straight-up. Clamps into [0, 1]; never panics.
fn max_tilt_magnitude(e_min_central_deg: f64) -> f64 {
    if ELEV_SPAN_DEG <= 0.0 {
        return 0.0; // degenerate: only straight up is safe
    }
    ((ELEV_AT_CENTER_DEG - e_min_central_deg) / ELEV_SPAN_DEG).clamp(0.0, 1.0)
}

/// The head's fan-roll state: continuous rotation, or parked at a roll angle.
enum RollState {
    /// Continuous rotation: the fan sweeps every roll angle.
    Spinning,
    /// Parked at an indexed roll position (fader value in 0..1).
    Parked(UnipolarFloat),
}

impl RollState {
    /// |sin phi| of the fan plane. Continuous rotation takes the worst case (1),
    /// since it sweeps every angle.
    fn abs_sin(&self) -> f64 {
        match self {
            Self::Spinning => 1.0,
            Self::Parked(roll) => {
                let phi_deg = ROLL_PHASE_DEG + roll.val() * ROLL_SPAN_DEG;
                phi_deg.to_radians().sin().abs()
            }
        }
    }
}

/// Beam-elevation floor from the group scale factor.
/// scale = 1 -> 0 (true horizon); scale < 1 -> raised safety cone.
fn beam_elevation_floor_deg(scale: UnipolarFloat) -> f64 {
    (1.0 - scale.val()) * ELEV_SPAN_DEG
}

/// Safe minimum CENTRAL-axis elevation for the current state: the lowest axis
/// elevation at which the fan's lowest beam still sits at or above the beam
/// floor, from exact spherical fan geometry, plus margin.
///
/// The lowest beam (fan-angle -alpha from the axis) has vertical component
/// cos(alpha)*sin(E) - |sin phi|*sin(alpha)*cos(E) = K*sin(E - psi). Solving
/// K*sin(E - psi) >= sin(floor) for the smallest axis elevation E gives
/// E = psi + asin(sin(floor)/K); the asin argument is clamped so an unreachable
/// floor pins the head straight up rather than panicking.
fn safe_min_axis_elevation_deg(scale: UnipolarFloat, roll: &RollState) -> f64 {
    let alpha = FAN_HALF_ANGLE_DEG.to_radians();
    let s = roll.abs_sin();
    let floor = beam_elevation_floor_deg(scale).to_radians();
    let k = (alpha.cos().powi(2) + s * s * alpha.sin().powi(2)).sqrt();
    let psi = (s * alpha.tan()).atan();
    let e_min = psi + (floor.sin() / k).clamp(-1.0, 1.0).asin();
    e_min.to_degrees() + TILT_FLOOR_MARGIN_DEG
}

/// Shared-floor dynamic rescale (symmetric tent): scale a normalized tilt `t`
/// about straight-up into the safe range [-t_max, +t_max], where t_max is the
/// largest magnitude keeping the lowest beam at/above the floor for the given
/// roll state. No dead zone: t = 0 -> straight up, t = -/+1 -> the safe horizon-ward
/// limit on each side. Compressing the whole input range into the band keeps any
/// value of `t` above the floor.
fn rescaled_tilt(t: BipolarFloat, scale: UnipolarFloat, roll: &RollState) -> BipolarFloat {
    let e_min = safe_min_axis_elevation_deg(scale, roll);
    let t_max = max_tilt_magnitude(e_min); // symmetric safe range [-t_max, +t_max]
    BipolarFloat::new(t.val() * t_max) // scale the throw about straight-up; no dead zone
}

/// Beam color index -> RGB triple, bounds-safe.
fn beam_rgb(index: usize) -> [u8; 3] {
    *BEAM_RGB.get(index).unwrap_or(&[0, 0, 0])
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

#[derive(Debug, EmitState, Control, DescribeControls, Update)]
pub struct Moonraker {
    // Offset 0: pan. Standard bipolar knob, mirrored, unclamped.
    #[channel_control]
    #[animate]
    pan: ChannelKnobBipolar<BipolarChannelMirror>,

    // Offset 1: tilt. SAFETY-CLAMPED. Non-mirrored bipolar knob whose render
    // strategy is `()` (state/OSC/knob only) — DMX is written by hand in
    // `render_with_animations` after the dynamic safe-band rescale. Non-mirrored
    // because the manual clamp needs a public post-animation accessor
    // (`val_with_anim`), which `Bipolar` exposes and `Mirrored` (private fields)
    // does not.
    #[channel_control]
    #[animate]
    tilt: ChannelKnobBipolar<Bipolar<()>>,

    // Offset 2 (shared, gated by `rotate`): continuous single-direction spin
    // speed ("Rotation"), rendered to 129..255 only in Rotation mode. Animatable.
    #[channel_control]
    #[animate]
    rotation: ChannelKnobUnipolar<UnipolarChannel>,

    /// Z-rotation mode toggle ("Rotate"): on = continuous Rotation, off = indexed Roll.
    // Gates offset 2 between `rotation` (continuous) and `roll` (indexed).
    rotate: Bool<()>,

    /// Indexed roll (park) angle of the beam fan.
    // OSC "Roll"; drives offset 2 (0..128) in Roll mode via a hand write under
    // the gate. Animatable — the clamp reads its post-animation value each frame.
    #[animate]
    roll: Unipolar<()>,

    // Offset 4: all-beams master on/off (255/0 only). Carries the global strobe
    // flash via `strobe_shutter()`. State-only bool + channel-level fader.
    #[channel_control]
    master: ChannelLevelBool<Bool<()>>,

    /// When on, every beam takes beam 1's color index (unison).
    link_all: Bool<()>,

    // Offsets 6/9/12/15/18/21 (+1,+2): the six beams' color selects. Each is an
    // 8-way index {Off,R,G,B,C,M,Y,W} expanded to a 3-byte on/off RGB triple by
    // hand; render strategy is `()`.
    beam1: IndexedSelect<()>,
    beam2: IndexedSelect<()>,
    beam3: IndexedSelect<()>,
    beam4: IndexedSelect<()>,
    beam5: IndexedSelect<()>,
    beam6: IndexedSelect<()>,

    /// Tilt-throw scale in [0, 1]: 1.0 reaches the true-horizon limit, smaller
    /// raises the beam-elevation floor into a cone, 0.0 pins the head straight up.
    // Per-group calibration data, not an OSC control (hence the skips).
    #[skip_control]
    #[skip_emit]
    tilt_range_scale: UnipolarFloat,
}

/// One validated group option: the tilt-throw scale.
#[derive(Deserialize, OptionsMenu)]
#[serde(deny_unknown_fields)]
pub struct GroupOptions {
    /// Tilt-throw scale in [0, 1]. 1.0 = full throw to the true-horizon safe
    /// limit on each side; smaller values raise the beam-elevation floor into a
    /// cone; 0.0 pins the head straight up.
    #[serde(
        default = "default_tilt_range_scale",
        deserialize_with = "crate::fixture::patch::deserialize_unipolar"
    )]
    pub tilt_range_scale: UnipolarFloat,
}

fn default_tilt_range_scale() -> UnipolarFloat {
    UnipolarFloat::ONE
}

impl PatchFixture for Moonraker {
    const NAME: FixtureType = FixtureType("Moonraker");
    type GroupOptions = GroupOptions;
    type PatchOptions = NoOptions;

    const PATCH_NOTES: &'static str = "Set fixture to 27-channel mode.";

    fn new(options: Self::GroupOptions) -> Self {
        Self {
            // Offset 0: pan, full 8-bit continuous, mirrored, knob 0.
            pan: Bipolar::channel("Pan", 0, 0, 255)
                .with_detent()
                .with_mirroring(true)
                .with_channel_knob(0),
            // Offset 1: tilt, state-only (`()`), detented, knob 1. Rendered by hand.
            tilt: Bipolar::new("Tilt", ()).with_detent().with_channel_knob(1),
            // Offset 2 (Rotation mode): spin speed 128..255, knob 2.
            rotation: Unipolar::channel("Rotation", 3, 255, 0).with_channel_knob(2),
            // Off = Roll (indexed), on = Rotation (continuous).
            rotate: Bool::new_off("Rotate", ()),
            // Offset 2 (Roll mode): park angle 0..128, TouchOSC fader.
            roll: Unipolar::new("Roll", ()),
            // Offset 4: master gate + strobe flash.
            master: Bool::new_off("Master", ()).with_channel_level(),
            link_all: Bool::new_off("LinkAll", ()),
            // Offsets 6/9/12/15/18/21: beam color indices (y-primary vertical grids).
            beam1: IndexedSelect::new("Beam1", 8, false, ()),
            beam2: IndexedSelect::new("Beam2", 8, false, ()),
            beam3: IndexedSelect::new("Beam3", 8, false, ()),
            beam4: IndexedSelect::new("Beam4", 8, false, ()),
            beam5: IndexedSelect::new("Beam5", 8, false, ()),
            beam6: IndexedSelect::new("Beam6", 8, false, ()),
            tilt_range_scale: options.tilt_range_scale,
        }
    }

    fn can_strobe() -> Option<StrobeResponse> {
        Some(StrobeResponse::Short)
    }

    fn new_patch(_: Self::GroupOptions, _: Self::PatchOptions) -> PatchConfig {
        PatchConfig {
            channel_count: 27,
            render_mode: None,
        }
    }
}

register_patcher!(Moonraker);
register_touchosc_template!(Moonraker);

impl AnimatedFixture for Moonraker {
    type Target = AnimationTarget;

    fn positioner_axes() -> Option<crate::positioner::PositionerAxes<Self::Target>> {
        Some(crate::positioner::PositionerAxes {
            x: AnimationTarget::Pan,
            y: AnimationTarget::Tilt,
            focus: None,
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
        // Offset 0: pan (auto-render).
        self.pan.render(
            group_controls,
            animation_vals.filter(&AnimationTarget::Pan),
            dmx_buf,
        );

        // Post-animation roll state. The safety clamp MUST use the post-animation
        // Roll value: an animated roll sweeps the fan plane, so the safe tilt
        // floor tracks it frame-by-frame (each frame stays safe).
        let roll = if self.rotate.val() {
            RollState::Spinning
        } else {
            RollState::Parked(
                self.roll
                    .val_with_anim(animation_vals.filter(&AnimationTarget::Roll)),
            )
        };

        // Offset 1: tilt — manual, safety rescaled. `self.tilt.control`'s
        // `val_with_anim` applies detent + animation + positioner Y offset; we
        // remap that post-animation value into the safe band (whose floor also
        // depends on the post-animation roll) and write DMX.
        let tilt_norm = self
            .tilt
            .control
            .val_with_anim(animation_vals.filter(&AnimationTarget::Tilt));
        let tilt_hw = rescaled_tilt(tilt_norm, self.tilt_range_scale, &roll);
        dmx_buf[1] = unipolar_to_range(0, 255, tilt_hw.rescale_as_unipolar());

        // Offset 2: shared roll channel. Spinning -> continuous 129..255; Parked
        // -> indexed 0..128 at the post-animation roll value.
        match &roll {
            RollState::Spinning => {
                dmx_buf[2] = 255;
                self.rotation.render(
                    group_controls,
                    animation_vals.filter(&AnimationTarget::Rotation),
                    dmx_buf,
                );
            }
            RollState::Parked(r) => dmx_buf[2] = unipolar_to_range(0, 105, *r),
        }

        // Offset 4: master, 255/0 only; flashes with the global strobe.
        let master_on = group_controls
            .strobe_shutter()
            .unwrap_or(self.master.control.val());
        dmx_buf[4] = if master_on { 255 } else { 0 };

        dmx_buf[5] = 0; // Offset 5: strobe — pinned open (0 and 255 both open).

        // Offsets 6..24: six beams, each a 3-byte RGB on/off triple. In link_all
        // mode every beam mirrors beam 1's index.
        let beams: [&IndexedSelect<()>; 6] = [
            &self.beam1,
            &self.beam2,
            &self.beam3,
            &self.beam4,
            &self.beam5,
            &self.beam6,
        ];
        let base = [6usize, 9, 12, 15, 18, 21];
        let link = self.link_all.val();
        for (beam, &b) in beams.iter().zip(base.iter()) {
            let idx = if link {
                self.beam1.selected()
            } else {
                beam.selected()
            };
            let rgb = beam_rgb(idx);
            dmx_buf[b] = rgb[0];
            dmx_buf[b + 1] = rgb[1];
            dmx_buf[b + 2] = rgb[2];
        }

        dmx_buf[24] = 0; // Offset 24: FX — safe/off.
        dmx_buf[25] = 0; // Offset 25: FX — safe/off.
        dmx_buf[26] = 0; // Offset 26: FX — safe/off.
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    const EPS: f64 = 1e-9;
    const FULL: UnipolarFloat = UnipolarFloat::ONE;

    /// The rescale's shape, independent of the fan angle: knob center points
    /// straight up, the rails are symmetric and actually move (no dead zone),
    /// and magnitude is monotonic.
    #[test]
    fn tilt_rescale_shape() {
        let spin = RollState::Spinning;
        let up = tilt_to_elevation_deg(rescaled_tilt(BipolarFloat::ZERO, FULL, &spin));
        assert!((up - ELEV_AT_CENTER_DEG).abs() < EPS); // center = straight up
        let rail_pos = rescaled_tilt(BipolarFloat::new(1.0), FULL, &spin).val();
        let rail_neg = rescaled_tilt(BipolarFloat::new(-1.0), FULL, &spin).val();
        assert!((rail_pos + rail_neg).abs() < EPS); // symmetric about straight-up
        assert!(rail_pos > 0.0); // rail moves off center — no dead zone
        let mid = rescaled_tilt(BipolarFloat::new(0.5), FULL, &spin).val();
        assert!(mid > 0.0 && mid < rail_pos); // strictly between
    }

    // Exact 3D lowest-beam elevation (deg) for a central axis at `e_axis_deg`
    // with the fan plane rolled by `phi_deg` — an independent brute-force check
    // on the clamp's own closed-form solve. A beam at fan-angle beta in
    // [-alpha, alpha] has vertical component
    // sin(e)cos(beta) + sin(phi)cos(e)sin(beta); minimize over beta.
    fn true_lowest_beam_elev_deg(e_axis_deg: f64, phi_deg: f64) -> f64 {
        let e = e_axis_deg.to_radians();
        let phi = phi_deg.to_radians();
        let alpha = FAN_HALF_ANGLE_DEG.to_radians();
        let n = 4000;
        let mut lo = f64::INFINITY;
        for i in 0..=n {
            let beta = -alpha + 2.0 * alpha * i as f64 / n as f64;
            let z = e.sin() * beta.cos() + phi.sin() * e.cos() * beta.sin();
            lo = lo.min(z);
        }
        lo.asin().to_degrees()
    }

    /// Safety-critical, checked against exact 3D geometry: the true lowest beam
    /// never crosses the horizon in any reachable roll/tilt/scale — including a
    /// cone reduction too aggressive to satisfy, where the head pins straight up
    /// (lowest beam at 90 - alpha).
    #[test]
    fn safety_clamp_keeps_beams_above_horizon() {
        for &scale in &[
            FULL,
            UnipolarFloat::new(0.5),
            UnipolarFloat::new(0.25),
            UnipolarFloat::ZERO,
        ] {
            for i in 0..=90 {
                let roll = UnipolarFloat::new(i as f64 / 90.0);
                let phi = ROLL_PHASE_DEG + roll.val() * ROLL_SPAN_DEG;
                let state = RollState::Parked(roll);
                for &t in &[BipolarFloat::new(-1.0), BipolarFloat::new(1.0)] {
                    let e_axis = tilt_to_elevation_deg(rescaled_tilt(t, scale, &state));
                    let low = true_lowest_beam_elev_deg(e_axis, phi);
                    assert!(
                        low >= -1e-6,
                        "parked phi={phi} scale={}: true beam {low:.5} below horizon",
                        scale.val()
                    );
                }
            }
            let e_axis = tilt_to_elevation_deg(rescaled_tilt(
                BipolarFloat::new(1.0),
                scale,
                &RollState::Spinning,
            ));
            assert!(true_lowest_beam_elev_deg(e_axis, 90.0) >= -1e-6);
        }
    }

    /// When the requested floor is achievable (floor <= 90 - alpha), the exact
    /// clamp is tight: at a horizon rail the true lowest beam sits on the floor.
    #[test]
    fn clamp_meets_achievable_floor() {
        let max_floor = ELEV_AT_CENTER_DEG - FAN_HALF_ANGLE_DEG;
        for &scale in &[FULL, UnipolarFloat::new(0.5)] {
            let floor = beam_elevation_floor_deg(scale);
            assert!(floor <= max_floor);
            for i in 0..=90 {
                let roll = UnipolarFloat::new(i as f64 / 90.0);
                let phi = ROLL_PHASE_DEG + roll.val() * ROLL_SPAN_DEG;
                let state = RollState::Parked(roll);
                let e_axis =
                    tilt_to_elevation_deg(rescaled_tilt(BipolarFloat::new(-1.0), scale, &state));
                let low = true_lowest_beam_elev_deg(e_axis, phi);
                assert!(
                    (low - floor).abs() < 1e-3,
                    "parked phi={phi} scale={}: beam {low:.4} != floor {floor}",
                    scale.val()
                );
            }
        }
    }

    #[test]
    fn beam_rgb_table() {
        assert_eq!(beam_rgb(0), [0, 0, 0]); // Off
        assert_eq!(beam_rgb(4), [0, 255, 255]); // Cyan
        assert_eq!(beam_rgb(7), [255, 255, 255]); // White
        assert_eq!(beam_rgb(99), [0, 0, 0]); // out of range -> off, no panic
    }
}
