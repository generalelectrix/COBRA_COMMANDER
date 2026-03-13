use anyhow::Result;
use eframe::egui;
use log::error;
use tunnels::clock_server::SharedClockData;
use zero_configure::pub_sub::SubscriberService;

use crate::clock_service::{browse_clock_providers, connect_to_provider, ClockService};
use crate::control::{CommandClient, MetaCommand};

/// Abstraction over clock provider discovery and connection.
pub(crate) trait ClockBrowser {
    fn list(&self) -> Vec<String>;
    fn connect(&self, name: &str) -> Result<ClockService>;
}

struct ZeroConfClockBrowser(SubscriberService<SharedClockData>);

impl ClockBrowser for ZeroConfClockBrowser {
    fn list(&self) -> Vec<String> {
        self.0.list()
    }
    fn connect(&self, name: &str) -> Result<ClockService> {
        connect_to_provider(&self.0, name)
    }
}

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
    clock_browser: Box<dyn ClockBrowser>,
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
            state: ClockConfigState::Configured {
                description: "Internal clocks (no audio)".to_string(),
            },
            clock_browser: Box::new(ZeroConfClockBrowser(browse_clock_providers(zmq_ctx))),
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
                        Self::ui_remote(ui, client, &*self.clock_browser, selected_provider)
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
        clock_browser: &dyn ClockBrowser,
        selected_provider: &mut Option<usize>,
    ) -> Option<ClockConfigState> {
        let providers = clock_browser.list();

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
            match clock_browser
                .connect(provider_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::mock::auto_respond_client;
    use egui_kittest::{Harness, kittest::Queryable};

    struct MockClockBrowser {
        providers: Vec<String>,
    }

    impl ClockBrowser for MockClockBrowser {
        fn list(&self) -> Vec<String> {
            self.providers.clone()
        }
        fn connect(&self, _name: &str) -> Result<ClockService> {
            Ok(ClockService::test_new())
        }
    }

    impl ClockPanel {
        fn test_new(audio_devices: Vec<String>, providers: Vec<String>) -> Self {
            Self {
                state: ClockConfigState::Choosing {
                    mode: ClockMode::Internal,
                    selected_audio: 0,
                    selected_provider: None,
                },
                clock_browser: Box::new(MockClockBrowser { providers }),
                audio_devices,
            }
        }
    }

    #[test]
    fn internal_apply_no_audio_transitions_to_configured() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel::test_new(vec![], vec![]),
        );

        harness.get_by_label("Apply").click();
        harness.run();

        let panel = harness.state();
        assert!(
            matches!(&panel.state, ClockConfigState::Configured { description } if description.contains("no audio")),
        );
    }

    #[test]
    fn internal_apply_with_audio_transitions_to_configured() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel::test_new(vec!["Built-in Mic".to_string()], vec![]),
        );

        harness.get_by_label("Apply").click();
        harness.run();

        let panel = harness.state();
        assert!(
            matches!(&panel.state, ClockConfigState::Configured { description } if description.contains("Built-in Mic")),
        );
    }

    #[test]
    fn configured_reconfigure_returns_to_choosing() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel {
                state: ClockConfigState::Configured {
                    description: "test config".to_string(),
                },
                clock_browser: Box::new(MockClockBrowser {
                    providers: vec![],
                }),
                audio_devices: vec![],
            },
        );

        harness.get_by_label("Reconfigure").click();
        harness.run();

        let panel = harness.state();
        assert!(matches!(&panel.state, ClockConfigState::Choosing { .. }));
    }

    #[test]
    fn remote_no_providers_shows_searching() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel::test_new(vec![], vec![]),
        );

        // Switch to Remote mode.
        harness.get_by_label("Remote Clock Service").click();
        // Use step() instead of run() because the spinner causes continuous repainting.
        harness.step();

        // Verify searching message is shown.
        assert!(harness.query_by_label("Searching for providers...").is_some());
    }

    #[test]
    fn remote_providers_listed_as_radio_buttons() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()]),
        );

        // Switch to Remote mode.
        harness.get_by_label("Remote Clock Service").click();
        harness.run();

        // Verify the provider appears.
        assert!(harness.query_by_label("clock-server-1").is_some());
    }

    #[test]
    fn remote_connect_disabled_until_selection() {
        let client = auto_respond_client();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client);
            },
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()]),
        );

        // Switch to Remote mode.
        harness.get_by_label("Remote Clock Service").click();
        harness.run();

        // Connect button should exist but be disabled (clicking it should not transition).
        let connect = harness.get_by_label("Connect");
        connect.click();
        harness.run();

        // Still in Choosing state — the disabled button did nothing.
        let panel = harness.state();
        assert!(matches!(&panel.state, ClockConfigState::Choosing { .. }));
    }
}
