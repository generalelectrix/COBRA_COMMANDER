//! Interact with a remote clock service.
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use anyhow::Result;
use log::error;
use tunnels::clock_server::{clock_subscriber, SharedClockData};
use tunnels_lib::prompt::{prompt_bool, prompt_parse};
use zmq::Context;

pub struct ClockService(Arc<Mutex<SharedClockData>>);

impl ClockService {
    pub fn get(&self) -> SharedClockData {
        let val = self.0.lock().unwrap();
        (*val).clone()
    }
}

/// Prompt the user to start the clock service.
/// If the user requests to start the service, browse briefly for services,
/// and present options.  Connect to the service and return a mutex that wraps
/// the clock state shared with the receiver thread.
pub fn prompt_start_clock_service(ctx: Context) -> Result<Option<ClockService>> {
    if !prompt_bool("Run clock service?")? {
        return Ok(None);
    }
    println!("Browsing for clock providers...");
    let service = clock_subscriber(ctx.clone());
    thread::sleep(Duration::from_secs(2));
    let providers = service.list();
    if providers.is_empty() {
        println!("No clock providers found.");
        // Recurse and ask again.
        return prompt_start_clock_service(ctx);
    }
    println!("Available clock providers:");
    for provider in &providers {
        println!("{provider}");
    }
    let provider = if providers.len() == 1 {
        providers[0].clone()
    } else {
        prompt_parse("Select a provider", |s| Ok(s.to_string()))?
    };
    let mut receiver = service.subscribe(&provider, None)?;
    let storage = Arc::new(Mutex::new(SharedClockData::default()));
    let storage_handle = storage.clone();
    thread::spawn(move || loop {
        let msg = match receiver.receive_msg(true) {
            Err(e) => {
                error!("clock receive error: {e}");
                continue;
            }
            Ok(None) => {
                continue;
            }
            Ok(Some(msg)) => msg,
        };
        let mut clock_state = storage_handle.lock().unwrap();
        *clock_state = msg;
    });
    println!("Connected to {provider}.");
    Ok(Some(ClockService(storage)))
}
