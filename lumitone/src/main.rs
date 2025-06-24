use anyhow::{anyhow, Context, Result};
use statrs::statistics::Statistics;
use std::{
    env::args,
    io::Write,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use log::info;

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

    let mut timing = vec![];

    info!("Test pattern: ramp brightness from 0 to full over a few seconds, sending a single parameter update every 20 ms.");

    for b in 0..=255 {
        let command = [Command::Brightness(b)];
        send(&controller, command, &mut timing)?;
        thread::sleep(Duration::from_millis(20));
    }

    log_timing(&timing);
    timing.clear();

    info!("Test pattern: ramp brightness from 0 to full over a few seconds, sending a full update every 20 ms.");
    for b in 0..=255 {
        let command = [
            Command::Speed(100),
            Command::Brightness(b),
            Command::SelectPalette(2),
        ];
        send(&controller, command, &mut timing)?;
        thread::sleep(Duration::from_millis(20));
    }

    log_timing(&timing);
    timing.clear();

    info!("Test pattern: update all parameters at 50 fps.");
    for b in 0..=255 {
        let command = [
            Command::Speed(b),
            Command::Brightness(b),
            Command::SelectPalette((b % 4) as usize),
        ];
        send(&controller, command, &mut timing)?;
        thread::sleep(Duration::from_millis(20));
    }

    log_timing(&timing);
    info!("Done.");
    Ok(())
}

fn send(
    controller: &Lumitone,
    msgs: impl IntoIterator<Item = Command>,
    timing: &mut Vec<Duration>,
) -> std::io::Result<()> {
    let start = Instant::now();
    controller.send(msgs)?;
    let end = start.elapsed();
    timing.push(end);
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
            Self::SelectPalette(v) => writeln!(w, "NEW_TUNING g_iWifiColorPal={v}"),
            Self::Brightness(v) => writeln!(w, "NEW_TUNING g_iWifiBrightness={v}"),
            Self::Speed(v) => writeln!(w, "NEW_TUNING g_iWifiSpeed={v}"),
        }
    }
}

pub struct Lumitone {
    addr: SocketAddr,
}

impl Lumitone {
    pub fn send(&self, msgs: impl IntoIterator<Item = Command>) -> std::io::Result<()> {
        let mut stream = TcpStream::connect(self.addr)?;
        for msg in msgs {
            msg.write(&mut stream)?;
        }
        writeln!(&mut stream)?;
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
