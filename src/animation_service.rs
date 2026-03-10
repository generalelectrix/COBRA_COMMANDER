use anyhow::Result;
use log::error;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tunnels::{animation::Animation, clock_server::SharedClockData};
use zero_configure::pub_sub::{PublisherService, SubscriberService};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AnimationServiceState {
    /// Animation state.
    pub animation: Animation,
    /// Clock state.
    pub clocks: SharedClockData,
    /// The number of fixtures patched in the current group.
    pub fixture_count: usize,
}

pub type AnimationPublisher = PublisherService<AnimationServiceState>;
pub type AnimationSubscriber = SubscriberService<AnimationServiceState>;

const SERVICE_NAME: &str = "current_animator_state";
const PORT: u16 = 9091;

pub fn animation_publisher(ctx: &zmq::Context) -> Result<AnimationPublisher> {
    PublisherService::new(ctx, SERVICE_NAME, PORT)
}

pub fn animation_subscriber(ctx: zmq::Context) -> AnimationSubscriber {
    SubscriberService::new(ctx, SERVICE_NAME.to_string())
}

pub fn start_service(
    ctx: zmq::Context,
    mut on_update: impl FnMut() + Send + 'static,
) -> Result<Arc<Mutex<AnimationServiceState>>> {
    println!("Browsing for animation providers...");
    let service = animation_subscriber(ctx);
    let provider = loop {
        thread::sleep(Duration::from_secs(2));
        let providers = service.list();
        if let Some(provider) = providers.into_iter().next() {
            break provider;
        } else {
            println!("No animation providers found; trying again.");
            continue;
        }
    };
    println!("Connecting to {provider}...");
    let mut receiver = service.subscribe(&provider, None)?;
    let storage = Arc::new(Mutex::new(AnimationServiceState {
        fixture_count: 1,
        ..Default::default()
    }));
    let storage_handle = storage.clone();
    thread::spawn(move || {
        loop {
            let msg = match receiver.receive_msg(true) {
                Err(e) => {
                    error!("animation receive error: {e}");
                    continue;
                }
                Ok(None) => {
                    continue;
                }
                Ok(Some(msg)) => msg,
            };
            *storage_handle.lock().unwrap() = msg;
            on_update();
        }
    });
    println!("Connected to {provider}.");
    Ok(storage)
}
