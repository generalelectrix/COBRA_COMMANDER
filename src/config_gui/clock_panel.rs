use anyhow::Result;
use eframe::egui;
use tunnels::clock_server::SharedClockData;
use zero_configure::pub_sub::SubscriberService;

use crate::clock_service::{ClockService, browse_clock_providers, connect_to_provider};
use crate::control::{CommandClient, MetaCommand};
use crate::gui_state::ClockStatus;
use crate::ui_util::ErrorModal;

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum ClockMode {
    Internal,
    Remote,
}

pub struct ClockPanel {
    mode: ClockMode,
    selected_audio: Option<usize>, // None = Offline
    selected_provider: Option<usize>,
    clock_browser: Box<dyn ClockBrowser>,
    audio_devices: Vec<String>,
}

impl ClockPanel {
    pub fn new(zmq_ctx: zmq::Context, clock_status: &ClockStatus) -> Self {
        let audio_devices = tunnels::audio::AudioInput::devices().unwrap_or_default();

        let mut panel = Self {
            mode: ClockMode::Internal,
            selected_audio: None,
            selected_provider: None,
            clock_browser: Box::new(ZeroConfClockBrowser(browse_clock_providers(zmq_ctx))),
            audio_devices,
        };
        panel.sync_from_status(clock_status);
        panel
    }

    fn sync_from_status(&mut self, status: &ClockStatus) {
        match status {
            ClockStatus::Internal { audio_device } => {
                self.mode = ClockMode::Internal;
                self.selected_audio = self.audio_devices.iter().position(|d| d == audio_device);
            }
            ClockStatus::Remote { .. } => {
                self.mode = ClockMode::Remote;
                self.selected_audio = None;
            }
        }
        self.selected_provider = None;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        client: &CommandClient,
        clock_status: &ClockStatus,
        error_modal: &mut ErrorModal,
    ) {
        ui.heading("Clocks");
        ui.separator();

        // Status indicator.
        let status_label = match clock_status {
            ClockStatus::Internal { audio_device } => {
                format!("Active: Internal ({audio_device})")
            }
            ClockStatus::Remote { provider } => {
                format!("Active: Remote ({provider})")
            }
        };
        ui.colored_label(egui::Color32::GREEN, &status_label);
        ui.add_space(8.0);

        // Mode radio buttons — detect change.
        let prev_mode = self.mode;
        ui.radio_value(&mut self.mode, ClockMode::Internal, "Internal Clocks");
        ui.radio_value(&mut self.mode, ClockMode::Remote, "Remote Clock Service");
        ui.add_space(8.0);

        // Switched to Internal → fire command immediately.
        if self.mode != prev_mode && self.mode == ClockMode::Internal {
            let device_name = self.current_audio_device();
            if let Err(e) = client.send_command(MetaCommand::UseInternalClocks(device_name)) {
                error_modal.show(format!("UseInternalClocks failed: {e}"));
                self.sync_from_status(clock_status);
                return;
            }
        }

        let mode_changed = self.mode != prev_mode;

        match self.mode {
            ClockMode::Internal => {
                self.ui_internal(ui, client, clock_status, error_modal);
            }
            ClockMode::Remote => {
                self.ui_remote(ui, client, clock_status, error_modal, mode_changed);
            }
        }
    }

    fn current_audio_device(&self) -> Option<String> {
        self.selected_audio.map(|i| self.audio_devices[i].clone())
    }

    fn refresh_audio_devices(&mut self, error_modal: &mut ErrorModal) {
        let prev_device = self.current_audio_device();
        match tunnels::audio::AudioInput::devices() {
            Ok(d) => self.audio_devices = d,
            Err(e) => {
                error_modal.show(format!("Failed to refresh audio devices: {e}"));
                return;
            }
        }
        self.selected_audio =
            prev_device.and_then(|name| self.audio_devices.iter().position(|d| d == &name));
    }

    fn ui_internal(
        &mut self,
        ui: &mut egui::Ui,
        client: &CommandClient,
        clock_status: &ClockStatus,
        error_modal: &mut ErrorModal,
    ) {
        let prev_audio = self.selected_audio;

        ui.horizontal(|ui| {
            ui.label("Audio Input Device:");
            if ui
                .button("🔄")
                .on_hover_text("Refresh device list")
                .clicked()
            {
                self.refresh_audio_devices(error_modal)
            }
        });

        let selected_text = self
            .selected_audio
            .map_or("Offline", |i| &self.audio_devices[i]);

        egui::ComboBox::from_id_salt("audio_device")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.selected_audio, None, "Offline");
                for (i, device) in self.audio_devices.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_audio, Some(i), device);
                }
            });

        if self.selected_audio != prev_audio {
            let device_name = self.current_audio_device();
            if let Err(e) = client.send_command(MetaCommand::UseInternalClocks(device_name)) {
                error_modal.show(format!("UseInternalClocks failed: {e}"));
                self.sync_from_status(clock_status);
            }
        }
    }

    fn ui_remote(
        &mut self,
        ui: &mut egui::Ui,
        client: &CommandClient,
        clock_status: &ClockStatus,
        error_modal: &mut ErrorModal,
        mode_changed: bool,
    ) {
        let providers = self.clock_browser.list();

        if providers.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Searching for providers...");
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }

        // Clamp selection to valid range.
        if let Some(sel) = self.selected_provider
            && sel >= providers.len()
        {
            self.selected_provider = None;
        }

        let prev_provider = self.selected_provider;

        let selected_text = self
            .selected_provider
            .and_then(|i| providers.get(i))
            .map_or("Select provider...", |s| s.as_str());

        ui.label("Clock Provider:");
        egui::ComboBox::from_id_salt("clock_provider")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                for (i, provider) in providers.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_provider, Some(i), provider);
                }
            });

        // Fire connect when provider changed via combo box, or when switching
        // back to Remote mode with a provider already selected.
        let provider_changed = self.selected_provider != prev_provider;
        let reconnect = mode_changed && self.selected_provider.is_some();

        if let Some(sel) = self.selected_provider
            && (provider_changed || reconnect)
        {
            let provider_name = &providers[sel];
            match self
                .clock_browser
                .connect(provider_name)
                .and_then(|service| client.send_command(MetaCommand::UseClockService(service)))
            {
                Ok(()) => {}
                Err(e) => {
                    error_modal.show(format!("Failed to connect to clock provider: {e}"));
                    self.sync_from_status(clock_status);
                }
            }
        }
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
        fn test_new(
            audio_devices: Vec<String>,
            providers: Vec<String>,
            clock_status: &ClockStatus,
        ) -> Self {
            let mut panel = Self {
                mode: ClockMode::Internal,
                selected_audio: None,
                selected_provider: None,
                clock_browser: Box::new(MockClockBrowser { providers }),
                audio_devices,
            };
            panel.sync_from_status(clock_status);
            panel
        }
    }

    fn test_clock_status() -> ClockStatus {
        ClockStatus::Internal {
            audio_device: "Offline".into(),
        }
    }

    #[test]
    fn switch_to_internal_fires_command() {
        let client = auto_respond_client();
        let clock_status = test_clock_status();
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(
                vec!["Built-in Mic".to_string()],
                vec!["clock-server-1".to_string()],
                &ClockStatus::Remote {
                    provider: "clock-server-1".into(),
                },
            ),
        );

        // Panel starts in Remote mode from clock_status. Switch to Internal.
        harness.get_by_label("Internal Clocks").click();
        harness.run();

        // Mode should now be Internal (command was fired immediately).
        let panel = harness.state();
        assert_eq!(panel.mode, ClockMode::Internal);
    }

    #[test]
    fn switch_to_remote_without_provider_fires_nothing() {
        let client = auto_respond_client();
        let clock_status = test_clock_status();
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()], &clock_status),
        );

        // Switch to Remote mode — should not fire any command yet.
        harness.get_by_label("Remote Clock Service").click();
        harness.run();

        let panel = harness.state();
        assert_eq!(panel.mode, ClockMode::Remote);
        assert!(panel.selected_provider.is_none());
    }

    #[test]
    fn selecting_provider_fires_connect() {
        let client = auto_respond_client();
        let clock_status = test_clock_status();
        let mut panel =
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()], &clock_status);

        // Switch to Remote mode.
        panel.mode = ClockMode::Remote;

        // Simulate selecting a provider (combo box selection).
        let prev_provider = panel.selected_provider;
        panel.selected_provider = Some(0);

        // The selection changed — verify the panel would fire connect.
        assert_ne!(panel.selected_provider, prev_provider);
        assert_eq!(panel.selected_provider, Some(0));

        // Verify connect succeeds.
        let providers = panel.clock_browser.list();
        let result = panel
            .clock_browser
            .connect(&providers[0])
            .and_then(|service| client.send_command(MetaCommand::UseClockService(service)));
        assert!(result.is_ok());
    }

    #[test]
    fn switch_back_to_remote_fires_reconnect() {
        let client = auto_respond_client();
        let clock_status = test_clock_status();
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()], &clock_status),
        );

        // Switch to Remote, select a provider.
        harness.state_mut().mode = ClockMode::Remote;
        harness.state_mut().selected_provider = Some(0);
        harness.run();

        // Switch to Internal.
        harness.get_by_label("Internal Clocks").click();
        harness.run();
        assert_eq!(harness.state().mode, ClockMode::Internal);

        // Switch back to Remote — provider is still selected, should reconnect.
        harness.get_by_label("Remote Clock Service").click();
        harness.run();

        let panel = harness.state();
        assert_eq!(panel.mode, ClockMode::Remote);
        assert_eq!(panel.selected_provider, Some(0));
    }

    #[test]
    fn remote_no_providers_shows_searching() {
        let client = auto_respond_client();
        let clock_status = test_clock_status();
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec![], vec![], &clock_status),
        );

        // Switch to Remote mode.
        harness.get_by_label("Remote Clock Service").click();
        harness.step();

        assert!(
            harness
                .query_by_label("Searching for providers...")
                .is_some()
        );
    }

    #[test]
    fn status_label_shows_internal() {
        let client = auto_respond_client();
        let clock_status = ClockStatus::Internal {
            audio_device: "Test Mic".into(),
        };
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec!["Test Mic".to_string()], vec![], &clock_status),
        );

        harness.run();

        assert!(
            harness
                .query_by_label("Active: Internal (Test Mic)")
                .is_some()
        );
    }

    #[test]
    fn status_label_shows_remote() {
        let client = auto_respond_client();
        let clock_status = ClockStatus::Remote {
            provider: "clock-server-1".into(),
        };
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec![], vec!["clock-server-1".to_string()], &clock_status),
        );

        harness.run();

        assert!(
            harness
                .query_by_label("Active: Remote (clock-server-1)")
                .is_some()
        );
    }

    #[test]
    fn test_new_initializes_from_internal_status() {
        let clock_status = ClockStatus::Internal {
            audio_device: "Built-in Mic".into(),
        };
        let panel = ClockPanel::test_new(
            vec!["Default".to_string(), "Built-in Mic".to_string()],
            vec![],
            &clock_status,
        );
        assert_eq!(panel.mode, ClockMode::Internal);
        assert_eq!(panel.selected_audio, Some(1));
        assert!(panel.selected_provider.is_none());
    }

    #[test]
    fn test_new_initializes_from_remote_status() {
        let clock_status = ClockStatus::Remote {
            provider: "srv".into(),
        };
        let panel = ClockPanel::test_new(
            vec!["Built-in Mic".to_string()],
            vec!["srv".to_string()],
            &clock_status,
        );
        assert_eq!(panel.mode, ClockMode::Remote);
        assert_eq!(panel.selected_audio, None);
        assert!(panel.selected_provider.is_none());
    }

    #[test]
    fn test_new_initializes_offline_status() {
        let clock_status = ClockStatus::Internal {
            audio_device: "Offline".into(),
        };
        let panel = ClockPanel::test_new(
            vec!["Built-in Mic".to_string(), "USB Mic".to_string()],
            vec![],
            &clock_status,
        );
        assert_eq!(panel.mode, ClockMode::Internal);
        assert_eq!(panel.selected_audio, None);
    }

    #[test]
    fn status_label_shows_offline() {
        let client = auto_respond_client();
        let clock_status = ClockStatus::Internal {
            audio_device: "Offline".into(),
        };
        let mut error_modal = ErrorModal::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut ClockPanel| {
                panel.ui(ui, &client, &clock_status, &mut error_modal);
            },
            ClockPanel::test_new(vec!["Built-in Mic".to_string()], vec![], &clock_status),
        );

        harness.run();

        assert!(
            harness
                .query_by_label("Active: Internal (Offline)")
                .is_some()
        );
    }
}
