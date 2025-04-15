use std::{env::args, io::Write, net::TcpStream, thread, time::Duration};

fn main() {
    let args: Vec<_> = args().collect();
    let dest_ip_str = args.get(1).expect("provide IP addr as sole CLI arg");

    let mut stream = TcpStream::connect(dest_ip_str).expect("failed to create TCP stream");

    println!("Sending test patterns to {}", stream.peer_addr().unwrap());

    println!("Test pattern: ramp brightness from 0 to full over a few seconds.");
    for b in 0..=255 {
        Command::Brightness(b).write(&mut stream).unwrap();
        thread::sleep(Duration::from_millis(10));
    }

    println!("Test pattern: ramp speed similarly.");
    for s in 0..=255 {
        Command::Speed(s).write(&mut stream).unwrap();
        thread::sleep(Duration::from_millis(10));
    }

    println!("Select palettes, let each run for 10 seconds.");
    for p in 0..4 {
        Command::SelectPalette(p).write(&mut stream).unwrap();
        thread::sleep(Duration::from_secs(10));
    }
    println!("Done.");
}

#[derive(Clone, Copy)]
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
