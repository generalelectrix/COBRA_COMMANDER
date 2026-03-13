use eframe::egui;
use log::error;
use tunnels::clock_server::SharedClockData;
use zero_configure::pub_sub::SubscriberService;

use crate::clock_service::{browse_clock_providers, connect_to_provider};
use crate::control::{CommandClient, MetaCommand};

#[derive(Clone, Copy, PartialEq)]
enum ClockMode {
    Internal,
    Remote,
}

enum ClockConfigState {
    /// User is choosing clock mode and options.
    Choosing {
        mode: ClockMode,
        selected_audio: usize,
        selected_provider: Option<usize>,
    },
    /// Successfully configured.
    Configured { description: String },
}

pub struct ClockPanel {
    state: ClockConfigState,
    /// Persistent — created at launch, browses forever.
    clock_subscriber: SubscriberService<SharedClockData>,
    /// Available audio input devices, populated once at construction.
    audio_devices: Vec<String>,
}

impl ClockPanel {
    pub fn new(zmq_ctx: zmq::Context) -> Self {
        let audio_devices = tunnels::audio::AudioInput::devices().unwrap_or_else(|e| {
            error!("Failed to list audio devices: {e}");
            vec![]
        });

        Self {
            state: ClockConfigState::Choosing {
                mode: ClockMode::Internal,
                selected_audio: 0,
                selected_provider: None,
            },
            clock_subscriber: browse_clock_providers(zmq_ctx),
            audio_devices,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, client: &CommandClient) {
        ui.heading("Clocks");
        ui.separator();

        // Helpers return an optional state transition to avoid double-mutable-borrow.
        let transition = match &mut self.state {
            ClockConfigState::Choosing {
                mode,
                selected_audio,
                selected_provider,
            } => {
                ui.radio_value(mode, ClockMode::Internal, "Internal Clocks");
                ui.radio_value(mode, ClockMode::Remote, "Remote Clock Service");
                ui.add_space(8.0);

                match *mode {
                    ClockMode::Internal => {
                        Self::ui_internal(ui, client, &self.audio_devices, selected_audio)
                    }
                    ClockMode::Remote => {
                        Self::ui_remote(ui, client, &self.clock_subscriber, selected_provider)
                    }
                }
            }
            ClockConfigState::Configured { description } => {
                ui.colored_label(egui::Color32::GREEN, format!("Configured: {description}"));
                if ui.button("Reconfigure").clicked() {
                    Some(ClockConfigState::Choosing {
                        mode: ClockMode::Internal,
                        selected_audio: 0,
                        selected_provider: None,
                    })
                } else {
                    None
                }
            }
        };

        if let Some(new_state) = transition {
            self.state = new_state;
        }
    }
}

impl ClockPanel {
    fn ui_internal(
        ui: &mut egui::Ui,
        client: &CommandClient,
        audio_devices: &[String],
        selected_audio: &mut usize,
    ) -> Option<ClockConfigState> {
        if audio_devices.is_empty() {
            ui.label("No audio input devices found.");
        } else {
            ui.label("Audio Input Device:");
            egui::ComboBox::from_id_salt("audio_device")
                .selected_text(&audio_devices[*selected_audio])
                .show_ui(ui, |ui| {
                    for (i, device) in audio_devices.iter().enumerate() {
                        ui.selectable_value(selected_audio, i, device);
                    }
                });
        }

        ui.add_space(8.0);

        if ui.button("Apply").clicked() {
            if !audio_devices.is_empty() {
                let device_name = audio_devices[*selected_audio].clone();
                match client.send_command(MetaCommand::SetAudioDevice(device_name.clone())) {
                    Ok(()) => {
                        return Some(ClockConfigState::Configured {
                            description: format!("Internal clocks (audio: {device_name})"),
                        });
                    }
                    Err(e) => {
                        error!("SetAudioDevice failed: {e}");
                    }
                }
            } else {
                return Some(ClockConfigState::Configured {
                    description: "Internal clocks (no audio)".to_string(),
                });
            }
        }
        None
    }

    fn ui_remote(
        ui: &mut egui::Ui,
        client: &CommandClient,
        clock_subscriber: &SubscriberService<SharedClockData>,
        selected_provider: &mut Option<usize>,
    ) -> Option<ClockConfigState> {
        let providers = clock_subscriber.list();

        if providers.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Searching for providers...");
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            return None;
        }

        // Clamp selection to valid range.
        if let Some(sel) = selected_provider
            && *sel >= providers.len()
        {
            *selected_provider = None;
        }

        for (i, provider) in providers.iter().enumerate() {
            let checked = *selected_provider == Some(i);
            if ui.radio(checked, provider).clicked() {
                *selected_provider = Some(i);
            }
        }

        ui.add_space(8.0);

        let has_selection = selected_provider.is_some();
        if ui
            .add_enabled(has_selection, egui::Button::new("Connect"))
            .clicked()
            && let Some(sel) = *selected_provider
        {
            let provider_name = &providers[sel];
            match connect_to_provider(clock_subscriber, provider_name)
                .and_then(|service| {
                    client.send_command(MetaCommand::UseClockService(service))
                })
            {
                Ok(()) => {
                    return Some(ClockConfigState::Configured {
                        description: format!(
                            "Remote clock service ({provider_name})"
                        ),
                    });
                }
                Err(e) => {
                    error!("Failed to connect to clock provider: {e}");
                }
            }
        }
        None
    }
}
