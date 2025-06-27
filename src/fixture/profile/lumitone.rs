//! Control profile for Lumitone.
use std::{
    io::Write,
    net::{SocketAddr, TcpStream},
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

use log::error;

use crate::fixture::prelude::*;

#[derive(Debug, EmitState, Control)]
pub struct Lumitone {
    #[channel_control]
    #[on_change = "update_state"]
    level: ChannelLevelUnipolar<Unipolar<()>>,
    #[channel_control]
    #[on_change = "update_state"]
    hue_coarse: ChannelKnobPhase<PhaseControl<()>>,
    #[channel_control]
    #[on_change = "update_state"]
    speed: ChannelKnobUnipolar<Unipolar<()>>,
    #[on_change = "update_state"]
    palette: IndexedSelect<()>,

    #[skip_control]
    #[skip_emit]
    send: Sender<Message>,

    // FIXME: we only need this because we use default for instantiating fixtures.
    // We should probably refactor fixture creation to force everything to
    // define the new method instead.
    #[skip_control]
    #[skip_emit]
    recv: Receiver<Message>,
}

impl Default for Lumitone {
    fn default() -> Self {
        let (send, recv) = channel();
        Self {
            level: Unipolar::new("Level", ()).with_channel_level(),
            hue_coarse: PhaseControl::new("HueCoarse", ()).with_channel_knob(0),
            speed: Unipolar::new("Speed", ()).with_channel_knob(1),
            palette: IndexedSelect::new("Palette", 12, false, ()),
            send,
            recv,
        }
    }
}

impl PatchFixture for Lumitone {
    fn channel_count(&self, render_mode: Option<RenderMode>) -> usize {
        0
    }

    fn new(options: &mut crate::config::Options) -> anyhow::Result<(Self, Option<RenderMode>)> {}
}

impl NonAnimatedFixture for Lumitone {
    fn render(&self, _: &FixtureGroupControls, _: &mut [u8]) {}
}

impl ControllableFixture for Lumitone {}

impl Lumitone {
    fn update_state(&self, emitter: &FixtureStateEmitter) {}
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
        writeln!(w, "NEW_TUNING g_iWifiHue={}", unit_to_u8(self.hue.val()))?;
        Ok(())
    }
}

fn unit_to_u8(v: f64) -> u8 {
    (255. * v).round() as u8
}

struct Palette {}

impl Palette {
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        // TODO
        Ok(())
    }
}

/// Handle sending messages to Lumitone, ensuring we don't send too frequently.
struct LumitoneSender {
    addr: SocketAddr,
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
        let mut stream = TcpStream::connect(self.addr)?;
        if let Some(state) = &self.pending_state {
            state.write(&mut stream)?;
        }
        if let Some(palette) = &self.pending_palette {
            palette.write(&mut stream)?;
        }
        writeln!(&mut stream)?;
        stream.flush()?;
        self.pending_state = None;
        self.pending_palette = None;
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
