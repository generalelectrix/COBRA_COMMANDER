use anyhow::Result;
use clap::Parser;
use log::LevelFilter;
use simplelog::{Config as LogConfig, SimpleLogger};

use crate::cli::Cli;

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
mod master;
mod midi;
mod osc;
mod preview;
mod show;
mod show_file;
mod strobe;
mod touchosc;
mod ui_util;
mod util;
mod wled;

fn main() -> Result<()> {
    let args = Cli::try_parse()?;

    let log_level = if args.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };

    SimpleLogger::init(log_level, LogConfig::default())?;

    #[cfg(target_os = "macos")]
    install_terminate_override();

    config_gui::run_console(args.osc_receive_port)
}
