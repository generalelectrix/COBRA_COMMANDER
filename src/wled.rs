//! Use a thread to perform asynchronous communication with a WLED instance.
use log::{debug, error, info, warn};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{Sender, channel},
    },
    time::Duration,
};

use reqwest::Url;
use wled_json_api_library::{structures::state::State, wled::Wled};

#[derive(Debug)]
pub struct WledController {
    url: Url,
    send: Sender<WledControlMessage>,
}

impl WledController {
    /// Run a thread to handle sending WLED control messages.
    ///
    /// Polls once every 5 seconds for initial configuration until the client
    /// responds.
    pub fn run(url: Url) -> Self {
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

        let init_url = url.clone();
        std::thread::spawn(move || {
            let mut wled = initialize(&init_url, Duration::from_secs(5));
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
                        debug!("Sending WLED state to {}.", init_url);
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
        Self {
            send: send_state,
            url,
        }
    }

    /// Send a WLED control message.
    pub fn send(&self, msg: WledControlMessage) {
        if self.send.send(msg).is_err() {
            error!(
                "WLED control channel hung up, unable to send message to {}.",
                self.url
            );
        }
    }
}

/// Poll the WLED API until we get a good config back.
fn initialize(url: &Url, poll_interval: Duration) -> Wled {
    info!("Initializing WLED for {url}...");
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
