use std::thread;

use anyhow::Result;
use slint::Model;
use tunnels::audio::AudioInput;
use tunnels::clock_server::{ClockProviderInfo as BrowseInfo, browse_clock_providers};

use crate::clock_service::connect_to_clock_provider;
use crate::control::{CommandClient, MetaCommand};

include!(concat!(env!("OUT_DIR"), "/config_panel.rs"));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Run a closure on the Slint event loop with access to the window.
/// Silently no-ops if the window has been closed.
fn update_ui(weak: &slint::Weak<ConfigPanel>, f: impl FnOnce(&ConfigPanel) + Send + 'static) {
    let weak = weak.clone();
    slint::invoke_from_event_loop(move || {
        if let Some(win) = weak.upgrade() {
            f(&win);
        }
    })
    .ok();
}

/// Build a Slint ModelRc from a Vec.
fn set_model<T: Clone + 'static>(items: Vec<T>) -> slint::ModelRc<T> {
    std::rc::Rc::new(slint::VecModel::from(items)).into()
}

/// Update the clock provider data model and sync the list view display items.
fn set_clock_providers(win: &ConfigPanel, providers: Vec<ClockProviderInfo>) {
    let items: Vec<slint::StandardListViewItem> = providers
        .iter()
        .map(|p| slint::StandardListViewItem::from(p.name.as_str()))
        .collect();
    win.set_clock_providers(set_model(providers));
    win.set_clock_provider_items(set_model(items));
}

/// Update the audio device data model and sync the list view display items.
fn set_audio_devices(win: &ConfigPanel, devices: Vec<slint::SharedString>) {
    let items: Vec<slint::StandardListViewItem> = devices
        .iter()
        .map(|d| slint::StandardListViewItem::from(d.as_str()))
        .collect();
    win.set_audio_devices(set_model(devices));
    win.set_audio_device_items(set_model(items));
}

// ---------------------------------------------------------------------------
// GUI entry point
// ---------------------------------------------------------------------------

pub fn run_gui(client: CommandClient, _universe_count: usize) -> Result<()> {
    let window = ConfigPanel::new()?;

    setup_clock_browsing(&window);
    setup_connect_clock(&window, &client);
    setup_audio_scan(&window);
    setup_audio_select(&window, &client);

    window.run()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Clock provider browsing (event-driven via DNS-SD)
// ---------------------------------------------------------------------------

fn setup_clock_browsing(window: &ConfigPanel) {
    let weak = window.as_weak();
    thread::spawn(move || {
        browse_clock_providers(
            {
                let weak = weak.clone();
                move |info: BrowseInfo| {
                    update_ui(&weak, move |win| {
                        let new = ClockProviderInfo {
                            name: info.name.into(),
                            host: info.host.into(),
                            port: info.port as i32,
                        };
                        let mut providers: Vec<ClockProviderInfo> = win
                            .get_clock_providers()
                            .iter()
                            .filter(|p| p.name != new.name)
                            .collect();
                        providers.push(new);
                        set_clock_providers(win, providers);
                    });
                }
            },
            {
                let weak = weak.clone();
                move |name: &str| {
                    let name = name.to_string();
                    update_ui(&weak, move |win| {
                        let providers: Vec<ClockProviderInfo> = win
                            .get_clock_providers()
                            .iter()
                            .filter(|p| p.name != name.as_str())
                            .collect();
                        set_clock_providers(win, providers);
                    });
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Connect to selected clock provider
// ---------------------------------------------------------------------------

fn setup_connect_clock(window: &ConfigPanel, client: &CommandClient) {
    let client = client.clone();
    let weak = window.as_weak();
    window.on_connect_clock_provider(move |index| {
        let provider = weak
            .upgrade()
            .unwrap()
            .get_clock_providers()
            .row_data(index as usize)
            .unwrap();
        let host = provider.host.to_string();
        let port = provider.port as u16;
        let name = provider.name.to_string();
        let client = client.clone();
        let weak = weak.clone();
        thread::spawn(move || {
            let result = connect_to_clock_provider(client.zmq_ctx(), &host, port)
                .and_then(|svc| client.send_command(MetaCommand::UseClockService(svc)));
            update_ui(&weak, move |win| match result {
                Ok(()) => win.set_clock_status(format!("Connected to {name}").into()),
                Err(e) => win.set_status_message(format!("Connection failed: {e}").into()),
            });
        });
    });
}

// ---------------------------------------------------------------------------
// Audio device scanning
// ---------------------------------------------------------------------------

fn setup_audio_scan(window: &ConfigPanel) {
    let weak = window.as_weak();
    window.on_scan_audio_devices(move || {
        let weak = weak.clone();
        thread::spawn(move || match AudioInput::devices() {
            Ok(devices) => update_ui(&weak, |win| {
                let model: Vec<slint::SharedString> = devices.into_iter().map(Into::into).collect();
                set_audio_devices(win, model);
            }),
            Err(e) => update_ui(&weak, move |win| {
                win.set_status_message(format!("Audio scan failed: {e}").into());
            }),
        });
    });
}

// ---------------------------------------------------------------------------
// Audio device selection
// ---------------------------------------------------------------------------

fn setup_audio_select(window: &ConfigPanel, client: &CommandClient) {
    let client = client.clone();
    let weak = window.as_weak();
    window.on_select_audio_device(move |index| {
        let device_name = weak
            .upgrade()
            .unwrap()
            .get_audio_devices()
            .row_data(index as usize)
            .unwrap()
            .to_string();
        match client.send_command(MetaCommand::SetAudioDevice(device_name.clone())) {
            Ok(()) => {
                if let Some(win) = weak.upgrade() {
                    win.set_status_message(format!("Audio: {device_name}").into());
                }
            }
            Err(e) => {
                if let Some(win) = weak.upgrade() {
                    win.set_status_message(format!("Error: {e}").into());
                }
            }
        }
    });
}
