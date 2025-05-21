use anyhow::{anyhow, Context, Result};
use statrs::statistics::Statistics;
use std::{
    env::args,
    io::Write,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use log::{debug, info};

fn main() -> Result<()> {
    simple_logger::init_with_env()?;
    let args: Vec<_> = args().collect();
    let dest_ip_str = args
        .get(1)
        .ok_or_else(|| anyhow!("provide socket addr as sole CLI arg"))?;

    let controller = Lumitone {
        addr: dest_ip_str
            .to_socket_addrs()
            .context("failed to parse socket address")?
            .next()
            .ok_or_else(|| anyhow!("no socket address parsed"))?,
    };
    info!("Sending test patterns to {}", controller.addr);

    info!("Test pattern: ramp brightness from 0 to full over a few seconds.");

    let mut timing = vec![];
    let send = |msg, timing: &mut Vec<Duration>| {
        debug!("Sending message: {msg:?}");
        let start = Instant::now();
        controller.send(&msg)?;
        let end = start.elapsed();
        timing.push(end);
        anyhow::Ok(())
    };

    for b in 0..=255 {
        send(Command::Brightness(b), &mut timing)?;
        thread::sleep(Duration::from_millis(10));
    }

    info!("Test pattern: ramp speed similarly.");
    for s in 0..=255 {
        send(Command::Speed(s), &mut timing)?;
        thread::sleep(Duration::from_millis(10));
    }

    info!("Select palettes, let each run for 10 seconds.");
    for p in 0..4 {
        send(Command::SelectPalette(p), &mut timing)?;
        thread::sleep(Duration::from_secs(10));
    }
    log_timing(&timing);
    timing.clear();
    info!("Stress test: spew messages as fast as possible.");
    for b in 0..=255 {
        send(Command::Brightness(b), &mut timing)?;
    }
    log_timing(&timing);
    info!("Done.");
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    SelectPalette(usize),
    Brightness(u8),
    Speed(u8),
}

impl Command {
    /// Write this command's payload into the provided writer.
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        match self {
            Self::SelectPalette(v) => write!(w, "NEW_TUNING g_iWifiColorPal={v}\n\n"),
            Self::Brightness(v) => write!(w, "NEW_TUNING g_iWifiBrightness={v}\n\n"),
            Self::Speed(v) => write!(w, "NEW_TUNING g_iWifiSpeed={v}\n\n"),
        }
    }
}

pub struct Lumitone {
    addr: SocketAddr,
}

impl Lumitone {
    pub fn send(&self, msg: &Command) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(self.addr)?;
        msg.write(&mut stream)?;
        stream.flush()?;
        Ok(())
    }
}

fn log_timing(times: &[Duration]) {
    let times_msecs: Vec<_> = times.iter().map(|d| d.as_secs_f64() * 1000.).collect();
    info!(
        "mean request time (ms): {}\nmin: {}\nmax: {}\nstddev: {}",
        Statistics::mean(&times_msecs),
        Statistics::min(&times_msecs),
        Statistics::max(&times_msecs),
        Statistics::std_dev(&times_msecs),
    );
}
