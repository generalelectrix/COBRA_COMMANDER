//! Use a thread to perform asynchronous communication with a WLED instance.
use log::{error, info, warn};
use std::{
    sync::{
        mpsc::{channel, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

use reqwest::Url;
use wled_json_api_library::{structures::state::State, wled::Wled};

use crate::control::ControlMessage;

pub struct WledController {
    send: Sender<WledControlMessage>,
}

impl WledController {
    /// Run a thread to handle sending WLED control messages.
    ///
    /// Polls once every 5 seconds for initial configuration until the client
    /// responds.
    pub fn run(url: Url, _send: Sender<ControlMessage>) -> Self {
        let (send_state, recv_state) = channel();

        let state = Arc::new(Mutex::new(None));
        let state_clone = state.clone();
        // Drain control channel using a thread into mutex.
        std::thread::spawn(move || {
            for msg in recv_state {
                let Ok(mut lock) = state_clone.lock() else {
                    error!("Failed to get WLED state lock.");
                    continue;
                };
                lock.replace(msg);
            }
            info!("WLED handler thread shutting down.");
        });

        std::thread::spawn(move || {
            let mut wled = initialize(&url, Duration::from_secs(5));
            let sleep = Duration::from_millis(100);
            loop {
                std::thread::sleep(sleep);
                let msg = {
                    let Ok(mut lock) = state.lock() else {
                        error!("Failed to get WLED state lock.");
                        continue;
                    };
                    let Some(state) = lock.take() else {
                        continue;
                    };
                    state
                };
                match msg {
                    WledControlMessage::SetState(state) => {
                        wled.state = Some(state);
                        info!("Sending state.");
                        if let Err(err) = wled.flush_state() {
                            error!("failed to send WLED state update: {err}");
                            continue;
                        }
                    }
                    WledControlMessage::GetEffectMetadata => {
                        // TODO
                        error!("fxdata not implemented");
                        continue;
                    }
                }
            }
        });
        Self { send: send_state }
    }
}

/// Poll the WLED API until we get a good config back.
fn initialize(url: &Url, poll_interval: Duration) -> Wled {
    info!("Initializing WLED...");
    loop {
        match Wled::try_from_url(url) {
            Ok(wled) => {
                info!("WLED client connection successful.");
                return wled;
            }
            Err(err) => {
                warn!("Failed to initialize WLED at {url}: {err}");
            }
        }
        std::thread::sleep(poll_interval);
    }
}

pub enum WledControlMessage {
    SetState(State),
    #[allow(unused)]
    GetEffectMetadata,
}

pub enum WledResponse {}

pub trait EmitWledControlMessage {
    fn emit_wled(&self, msg: WledControlMessage);
}

impl EmitWledControlMessage for WledController {
    fn emit_wled(&self, msg: WledControlMessage) {
        if self.send.send(msg).is_err() {
            error!("Error emitting WLED control message: message processor channel has hung up.");
        }
    }
}
