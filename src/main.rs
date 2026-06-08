use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

/// Backlog of in-flight log records between the producer and the drain thread.
/// Records are dropped (and counted) when this fills, so logging never blocks
/// a real-time thread.
const LOG_CHANNEL_CAPACITY: usize = 1024;

/// Per-severity scrollback retained for the in-GUI log view.
const LOG_SCROLLBACK_PER_SEVERITY: usize = 500;

/// Override NSApplication's terminate: to send performClose: to the key
/// window instead of killing the process. This converts Cmd+Q into the
/// same close event as clicking the red window button, which our
/// CloseHandler can intercept with a confirmation dialog.
#[cfg(target_os = "macos")]
fn install_terminate_override() {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::sel;

    unsafe extern "C" fn terminate_override(
        _this: *mut AnyObject,
        _cmd: Sel,
        _sender: *mut AnyObject,
    ) {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;

        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let app = NSApplication::sharedApplication(mtm);
        if let Some(window) = app.keyWindow() {
            unsafe { window.performClose(None) };
        }
    }

    unsafe {
        let class = AnyClass::get("NSApplication").expect("NSApplication class not found");
        let method = class
            .instance_method(sel!(terminate:))
            .expect("terminate: method not found");
        let imp: Imp = std::mem::transmute(terminate_override as *mut ());
        method.set_implementation(imp);
    }
}

mod animation;
mod channel;
mod cli;
mod clock_service;
mod clocks;
mod color;
mod config;
mod config_gui;
mod control;
mod dmx;
mod fixture;
mod gui_state;
mod local_ip_watch;
mod master;
mod midi;
mod osc;
mod positioner;
mod preview;
mod show;
mod show_file;
mod show_saver;
mod strobe;
mod touchosc;
mod ui_util;
mod util;
mod wled;
mod worker;

fn main() -> Result<()> {
    let args = Cli::try_parse()?;

    // The in-GUI Status view is the only log destination — no stderr/terminal output.
    // The sink captures whatever passes the global gate; the GUI "Capture" dropdown owns
    // that gate via `log::set_max_level`.
    let (capture, log_rx) = gui_common::log_status::channel(LOG_CHANNEL_CAPACITY);
    log::set_boxed_logger(Box::new(capture))?;
    log::set_max_level(if args.debug {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Warn
    });

    #[cfg(target_os = "macos")]
    install_terminate_override();

    config_gui::run_console(log_rx)
}
