//! Control profile for Lumitone.
use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

use anyhow::Context;
use log::error;

use crate::{
    color::{ColorRgb, ColorSpace},
    fixture::{
        color::{Color, Model},
        prelude::*,
    },
};

const SOCKET_OPT: &str = "socket";

#[derive(Debug, EmitState, Control, Update)]
pub struct Lumitone {
    #[channel_control]
    #[on_change = "send_state"]
    level: ChannelLevelUnipolar<Unipolar<()>>,
    #[channel_control]
    #[on_change = "send_state"]
    hue_coarse: ChannelKnobPhase<PhaseControl<()>>,
    #[channel_control]
    #[on_change = "send_state"]
    speed: ChannelKnobUnipolar<Unipolar<()>>,
    #[on_change = "update_for_palette_select"]
    palette: IndexedSelect<()>,

    // Fine hue adjust - used for preset complex palettes.
    #[on_change = "update_hue_fine"]
    hue_fine: Bipolar<()>,

    // Controls for custom palette creation.
    #[on_change = "send_custom_palette"]
    color0: Color,
    #[on_change = "send_custom_palette"]
    color1: Color,
    #[on_change = "send_custom_palette"]
    color2: Color,
    #[on_change = "send_custom_palette"]
    color3: Color,
    #[on_change = "send_custom_palette"]
    color4: Color,

    #[skip_control]
    #[skip_emit]
    send: Sender<Message>,

    #[skip_control]
    #[skip_emit]
    per_palette_hue_adjust: Vec<BipolarFloat>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupOptions {
    socket: Option<SocketAddr>,
}

impl PatchFixture for Lumitone {
    const NAME: FixtureType = FixtureType("Lumitone");

    fn new(options: Options) -> Result<Self> {
        // Instantiate the control sender.
        let options: GroupOptions = options.parse()?;
        let (send, recv) = channel();

        let l = Self {
            level: Unipolar::new("Level", ()).with_channel_level(),
            hue_coarse: PhaseControl::new("HueCoarse", ()).with_channel_knob(0),
            speed: Unipolar::new("Speed", ()).with_channel_knob(1),
            palette: IndexedSelect::new("Palette", PALETTE_COUNT, false, ()),
            hue_fine: Bipolar::new("HueFine", ()),
            per_palette_hue_adjust: vec![BipolarFloat::ZERO; PALETTE_COUNT],
            color0: Color::for_subcontrol(Some(0), ColorSpace::Hsv),
            color1: Color::for_subcontrol(Some(1), ColorSpace::Hsv),
            color2: Color::for_subcontrol(Some(2), ColorSpace::Hsv),
            color3: Color::for_subcontrol(Some(3), ColorSpace::Hsv),
            color4: Color::for_subcontrol(Some(4), ColorSpace::Hsv),
            send,
        };

        std::thread::spawn(move || {
            let sender = LumitoneSender {
                addr: options.socket,
                recv,
                pending_state: None,
                pending_palette: None,
                last_send: Instant::now(),
            };
            if let Err(err) = sender.run() {
                error!("{err}");
            }
        });

        Ok(l)
    }

    fn group_options() -> Vec<(String, PatchOption)> {
        vec![(SOCKET_OPT.to_string(), PatchOption::SocketAddr)]
    }

    fn patch_options() -> Vec<(String, PatchOption)> {
        vec![]
    }
}

impl CreatePatchConfig for Lumitone {
    fn patch(&self, options: Options) -> Result<PatchConfig> {
        options.ensure_empty()?;
        Ok(PatchConfig {
            channel_count: 0,
            render_mode: None,
        })
    }
}

register_patcher!(Lumitone);

impl NonAnimatedFixture for Lumitone {
    fn render(&self, _: &FixtureGroupControls, _: &mut [u8]) {}
}

const SIMPLE_PALETTE_INDEX: usize = 7;
const SIMPLE_PALETTE_WITH_WHITE_INDEX: usize = 8;
const CUSTOM_PALETTE_INDEX: usize = 11;
const PALETTE_COUNT: usize = 12;
const FINE_HUE_ADJUST_SCALE: f64 = 0.1;

impl Lumitone {
    /// Get the current value of the hue control based on the selected palette.
    fn current_hue(&self) -> Phase {
        match self.palette.selected() {
            (0..SIMPLE_PALETTE_INDEX) => {
                // complex palette; use the fine hue adjust
                Phase::new(self.hue_fine.val().val() * FINE_HUE_ADJUST_SCALE)
            }
            SIMPLE_PALETTE_INDEX | SIMPLE_PALETTE_WITH_WHITE_INDEX | 9 | 10 => {
                // simple palette; use the coarse hue adjust
                // simple palettes are red by default; this adjustment ensures we
                // match the hue controls in other fixtures
                self.hue_coarse.control.val() + Phase::new(1. / 3.)
            }
            CUSTOM_PALETTE_INDEX => {
                // custom palette; no hue adjust
                Phase::ZERO
            }
            out_of_range => {
                error!("unexpected out of range Lumitone palette: {out_of_range}");
                Phase::ZERO
            }
        }
    }

    /// Perform necessary actions when we've selected a new palette.
    fn update_for_palette_select(&mut self, emitter: &FixtureStateEmitter) {
        // Load the existing fine hue adjust for this palette.
        let fine_adjust = self
            .per_palette_hue_adjust
            .get(self.palette.selected())
            .copied()
            .unwrap_or_default();
        let _ = self.hue_fine.control_direct(fine_adjust, emitter);

        // Update the full output state.
        self.send_state(emitter);
    }

    /// Store fine hue adjustments per-palette.
    ///
    /// Don't do this for palettes where we're not using fine hue adjust.
    fn update_hue_fine(&mut self, emitter: &FixtureStateEmitter) {
        let palette = self.palette.selected();
        if !(0..SIMPLE_PALETTE_INDEX).contains(&palette) {
            // Revert the fine hue control to make it clear that it isn't doing anything.
            let _ = self.hue_fine.control_direct(BipolarFloat::ZERO, emitter);
            return;
        }
        let Some(adj) = self.per_palette_hue_adjust.get_mut(self.palette.selected()) else {
            return;
        };
        *adj = self.hue_fine.val();

        // Update the full output state.
        self.send_state(emitter);
    }

    fn send_state(&self, _emitter: &FixtureStateEmitter) {
        if self
            .send
            .send(Message::State(State {
                level: self.level.control.val(),
                hue: self.current_hue(),
                speed: self.speed.control.val(),
                palette: self.palette.selected(),
            }))
            .is_err()
        {
            error!("Cannot send Lumitone state update; sender disconnected.");
        }
    }

    fn send_custom_palette(&self, _emitter: &FixtureStateEmitter) {
        let mut p = Palette::default();
        self.color0
            .render_without_animations(Model::Rgb, &mut p.color0);
        self.color1
            .render_without_animations(Model::Rgb, &mut p.color1);
        self.color2
            .render_without_animations(Model::Rgb, &mut p.color2);
        self.color3
            .render_without_animations(Model::Rgb, &mut p.color3);
        self.color4
            .render_without_animations(Model::Rgb, &mut p.color4);
        if self.send.send(Message::CustomPalette(p)).is_err() {
            error!("Cannot send Lumitone custom palette update; sender disconnected.");
        }
    }
}

enum Message {
    State(State),
    CustomPalette(Palette),
}

struct State {
    level: UnipolarFloat,
    hue: Phase,
    speed: UnipolarFloat,
    palette: usize,
}

impl State {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        writeln!(w, "NEW_TUNING g_iWifiColorPal={}", self.palette as u8)?;
        writeln!(
            w,
            "NEW_TUNING g_iWifiBrightness={}",
            unit_to_u8(self.level.val())
        )?;
        writeln!(
            w,
            "NEW_TUNING g_iWifiSpeed={}",
            unit_to_u8(self.speed.val())
        )?;
        writeln!(
            w,
            "NEW_TUNING g_iWifiHue={}",
            phase_to_u8_hue(self.hue.val())
        )?;
        Ok(())
    }
}

fn unit_to_u8(v: f64) -> u8 {
    (255. * v).round() as u8
}

/// Lumitone expects a 0 hue offset to come out as 127.
fn phase_to_u8_hue(v: f64) -> u8 {
    ((255. * v).round() + 127.).rem_euclid(255.) as u8
}

#[derive(Default)]
struct Palette {
    color0: ColorRgb,
    color1: ColorRgb,
    color2: ColorRgb,
    color3: ColorRgb,
    color4: ColorRgb,
}

impl Palette {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        fn write_color(
            heat_index: u8,
            [r, g, b]: ColorRgb,
            w: &mut impl Write,
        ) -> std::io::Result<()> {
            write!(w, "{heat_index},{r},{g},{b}")
        }
        write!(w, "NEW_COLOR_GRAD Index={CUSTOM_PALETTE_INDEX} PalHeat=")?;
        write_color(0, self.color0, &mut w)?;
        write!(w, ",")?;
        write_color(100, self.color1, &mut w)?;
        write!(w, ",")?;
        write_color(150, self.color2, &mut w)?;
        write!(w, ",")?;
        write_color(200, self.color3, &mut w)?;
        write!(w, ",")?;
        write_color(255, self.color4, &mut w)?;
        write!(w, "\n\n")?;
        Ok(())
    }
}

/// Handle sending messages to Lumitone, ensuring we don't send too frequently.
struct LumitoneSender {
    /// Socket address to send messages to.
    /// If None, log send events instead of sending.
    addr: Option<SocketAddr>,
    recv: Receiver<Message>,
    pending_state: Option<State>,
    pending_palette: Option<Palette>,
    last_send: Instant,
}

impl LumitoneSender {
    const SEND_INTERVAL: Duration = Duration::from_millis(20);

    /// Run this sender in the current thread as long as the channel is open.
    fn run(mut self) -> anyhow::Result<()> {
        loop {
            if !self.dirty() {
                // No pending state; wait indefinitely for an update.
                self.recv()?;
            }

            let time_until_next_send = Self::SEND_INTERVAL
                .checked_sub(self.last_send.elapsed())
                .unwrap_or_default();

            if time_until_next_send.is_zero() {
                // We have pending state and it's time to send it.
                // Regardless of if send succeeds or fails, don't try again
                // too soon.
                self.last_send = Instant::now();
                if let Err(err) = self.send() {
                    error!("Lumitone send error: {err}.");
                }
            } else {
                // Finite time until we should send. Keep updating our
                // state until then.
                self.recv_timeout(time_until_next_send)?;
            }
        }
    }

    /// Return true if we have unsent state updates.
    fn dirty(&self) -> bool {
        self.pending_state.is_some() || self.pending_palette.is_some()
    }

    /// Send our current state.
    ///
    /// If the send succeeds, wipe that state.
    fn send(&mut self) -> std::io::Result<()> {
        if let Some(addr) = &self.addr {
            self.write_into(TcpStream::connect(addr)?)?;
        } else {
            let mut buf = vec![];
            self.write_into(&mut buf)?;
            println!(
                "Lumitone control message: {}",
                String::from_utf8(buf).unwrap_or_default()
            );
        }
        self.pending_state = None;
        self.pending_palette = None;
        Ok(())
    }

    /// Write the current Lumitone control state into the provided writer.
    ///
    /// Flushes the writer.
    fn write_into(&self, mut w: impl Write) -> std::io::Result<()> {
        if let Some(state) = &self.pending_state {
            state.write(&mut w)?;
        }
        if let Some(palette) = &self.pending_palette {
            palette.write(&mut w)?;
        }
        writeln!(&mut w)?;
        w.flush()?;
        Ok(())
    }

    /// Wait indefinitely for a state update.
    fn recv(&mut self) -> anyhow::Result<()> {
        let Ok(msg) = self.recv.recv() else {
            bail!("Lumitone channel hung up, terminating sender thread");
        };
        self.update(msg);
        Ok(())
    }

    /// Wait at most timeout for a state update.
    fn recv_timeout(&mut self, timeout: Duration) -> anyhow::Result<()> {
        match self.recv.recv_timeout(timeout) {
            Ok(msg) => {
                self.update(msg);
                Ok(())
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("Lumitone channel hung up, terminating sender thread");
            }
            Err(RecvTimeoutError::Timeout) => Ok(()),
        }
    }

    fn update(&mut self, msg: Message) {
        match msg {
            Message::State(state) => {
                self.pending_state = Some(state);
            }
            Message::CustomPalette(palette) => {
                self.pending_palette = Some(palette);
            }
        };
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_state_write() {
        let state = State {
            level: UnipolarFloat::ONE,
            hue: Phase::ZERO,
            speed: UnipolarFloat::new(0.25),
            palette: 2,
        };
        let mut buf = vec![];
        state.write(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            "NEW_TUNING g_iWifiColorPal=2
NEW_TUNING g_iWifiBrightness=255
NEW_TUNING g_iWifiSpeed=64
NEW_TUNING g_iWifiHue=127
",
            s,
        );
    }

    #[test]
    fn test_palette_write() {
        let p = Palette {
            color0: [1, 2, 3],
            color1: [4, 5, 6],
            color2: [7, 8, 9],
            color3: [10, 11, 12],
            color4: [13, 14, 15],
        };
        let mut buf = vec![];
        p.write(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            "NEW_COLOR_GRAD Index=11 PalHeat=0,1,2,3,100,4,5,6,150,7,8,9,200,10,11,12,255,13,14,15\n\n",
            s,
        );
    }
}
