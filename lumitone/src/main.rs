use std::{env::args, io::Write, net::TcpStream, thread, time::{Duration, SystemTime}};
use chrono::{Local};


#[derive(Clone, Copy, Debug)]
pub enum Command {
    SelectPalette(usize),
    Brightness(u8),
    Speed(u8),
}


fn main() {
    let args: Vec<_> = args().collect();

    let verbose = args.iter().any(|arg| arg == "--verbose");

    // Find first argument that looks like an IP:PORT (naively checks for ':')
    let dest_ip_str = args.iter()
        .find(|arg| arg.contains(':') && !arg.starts_with("--"))
        .expect("provide IP addr as CLI arg (e.g., 192.168.1.159:848)");

    println!("Verbose logging is {}", if verbose { "ON" } else { "OFF" });

    println!("Sending brightness 0");
    send_command(dest_ip_str, Command::Brightness(0), verbose);
    thread::sleep(Duration::from_millis(4000));

    println!("Test pattern: ramp brightness from 0 to full over a few seconds.");
    for b in 0..=255 {
        send_command(dest_ip_str, Command::Brightness(b), verbose);
        thread::sleep(Duration::from_millis(10));
    }

    println!("Test pattern: ramp speed similarly.");
    for s in 0..=255 {
        send_command(dest_ip_str, Command::Speed(s), verbose);
        thread::sleep(Duration::from_millis(10));
    }

    println!("Select palettes, let each run for 10 seconds.");
    for p in 0..4 {
        send_command(dest_ip_str, Command::SelectPalette(p), verbose);
        thread::sleep(Duration::from_secs(2));
    }

    println!("Done.");
}


impl Command {
    /// Format this command into a string to be sent over the socket.
    pub fn to_string(&self) -> String {
        match self {
            Self::SelectPalette(v) => format!("NEW_TUNING g_iWifiColorPal={}\n", v),
            Self::Brightness(v) => format!("NEW_TUNING g_iWifiBrightness={}\n", v),
            Self::Speed(v) => format!("NEW_TUNING g_iWifiSpeed={}\n", v),
        }
    }

    /// Send the command through the provided writer.
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        let msg = self.to_string();
        w.write_all(msg.as_bytes())?;
        w.flush()?;
        Ok(())
    }
}


fn timestamp() -> String {
    let now = SystemTime::now();
    let datetime: chrono::DateTime<Local> = now.into();
    format!("{}", datetime.format("%H:%M:%S%.3f"))
}


pub fn send_command(ip: &str, command: Command, verbose: bool) {
    let msg = command.to_string();

    println!("Connecting to {}... Sending command: {:?}", ip, command);

    match TcpStream::connect(ip) {
        Ok(mut stream) => {
            if verbose {
                println!("[{}] Connected.", timestamp());
                println!("[{}] Raw message: {:?}", timestamp(), msg);
            }

            match command.write(&mut stream) {
                Ok(()) => {
                    if verbose {
                        println!("[{}] Command sent successfully.", timestamp());
                    }
                }
                Err(e) => {
                    eprintln!("[{}] Failed to write command: {}", timestamp(), e);
                }
            }

            if verbose {
                println!("[{}] Done with command: {:?}\n", timestamp(), command);
            }
        }
        Err(e) => {
            eprintln!("[{}] Connection failed: {}\n", timestamp(), e);
        }
    }
}
