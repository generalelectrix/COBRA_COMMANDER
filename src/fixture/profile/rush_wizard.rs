//! Martin Rush-series Wizard (still not as good as the OG).

use crate::fixture::prelude::*;
use crate::osc::prelude::*;

#[derive(Debug)]
pub struct RushWizard {
    dimmer: UnipolarChannelLevel<UnipolarChannel>,
    strobe: StrobeChannel,
    color: LabeledSelect,
    twinkle: Bool<()>,
    twinkle_speed: UnipolarChannel,
    gobo: IndexedSelectMult,
    drum_rotation: BipolarSplitChannelMirror,
    drum_swivel: BipolarChannelMirror,
    reflector_rotation: BipolarSplitChannelMirror,
}

impl Default for RushWizard {
    fn default() -> Self {
        Self {
            dimmer: Unipolar::full_channel("Dimmer", 1).with_channel_level(),
            strobe: Strobe::channel("Strobe", 0, 16, 131, 8),
            color: LabeledSelect::new(
                "Color",
                2,
                vec![
                    ("Open", 159),
                    ("Blue", 161),
                    ("Magenta", 164),
                    ("Yellow", 167),
                    ("DarkBlue", 170),
                    ("White", 173),
                    ("Red", 176),
                    ("Orange", 179),
                    ("Green", 182),
                ],
            ),
            twinkle: Bool::new_off("Twinkle", ()),
            twinkle_speed: Unipolar::channel("TwinkleSpeed", 2, 221, 243),
            // 16 gobos, including the open position
            gobo: IndexedSelect::multiple("Gobo", 3, false, 16, 2, 160),
            drum_rotation: Bipolar::split_channel("DrumRotation", 4, 190, 128, 193, 255, 191)
                .with_mirroring(true),
            drum_swivel: Bipolar::channel("DrumSwivel", 5, 0, 120).with_mirroring(true),
            reflector_rotation: Bipolar::split_channel(
                "ReflectorRotation",
                6,
                190,
                128,
                193,
                255,
                191,
            )
            .with_mirroring(true),
        }
    }
}

impl PatchFixture for RushWizard {
    const NAME: FixtureType = FixtureType("RushWizard");
    fn channel_count(&self) -> usize {
        10
    }
}

impl NonAnimatedFixture for RushWizard {
    fn render(&self, group_controls: &FixtureGroupControls, dmx_buf: &mut [u8]) {
        self.strobe
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.dimmer.render_no_anim(dmx_buf);
        if self.twinkle.val() {
            self.twinkle_speed.render_no_anim(dmx_buf);
        } else {
            self.color.render_no_anim(dmx_buf);
        }
        self.gobo.render_no_anim(dmx_buf);
        self.drum_rotation
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.drum_swivel
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        self.reflector_rotation
            .render_with_group(group_controls, std::iter::empty(), dmx_buf);
        dmx_buf[7] = 0;
        dmx_buf[8] = 0;
        dmx_buf[9] = 0;
    }
}

impl ControllableFixture for RushWizard {
    fn emit_state(&self, emitter: &FixtureStateEmitter) {
        self.dimmer.emit_state(emitter);
        self.strobe.emit_state(emitter);
        self.color.emit_state(emitter);
        self.twinkle.emit_state(emitter);
        self.twinkle_speed.emit_state(emitter);
        self.gobo.emit_state(emitter);
        self.drum_rotation.emit_state(emitter);
        self.drum_swivel.emit_state(emitter);
        self.reflector_rotation.emit_state(emitter);
    }

    fn control(
        &mut self,
        msg: &OscControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<bool> {
        if self.dimmer.control(msg, emitter)? {
            return Ok(true);
        }
        if self.strobe.control(msg, emitter)? {
            return Ok(true);
        }
        if self.color.control(msg, emitter)? {
            return Ok(true);
        }
        if self.twinkle.control(msg, emitter)? {
            return Ok(true);
        }
        if self.twinkle_speed.control(msg, emitter)? {
            return Ok(true);
        }
        if self.gobo.control(msg, emitter)? {
            return Ok(true);
        }
        if self.drum_rotation.control(msg, emitter)? {
            return Ok(true);
        }
        if self.drum_swivel.control(msg, emitter)? {
            return Ok(true);
        }
        if self.reflector_rotation.control(msg, emitter)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn control_from_channel(
        &mut self,
        msg: &ChannelControlMessage,
        emitter: &FixtureStateEmitter,
    ) -> anyhow::Result<()> {
        self.dimmer.control_from_channel(msg, emitter)?;
        Ok(())
    }
}
