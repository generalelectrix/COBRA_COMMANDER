use std::time::Duration;

use gui_common::audio_panel::{
    AudioCommands, AudioPanel as SharedAudioPanel, AudioPanelState as SharedAudioPanelState,
    AudioSnapshot,
};
use tunnels::audio::processor::TrackingMode;
use tunnels::audio::{AudioInput, ControlMessage, StateChange};

use crate::control::MetaCommand;
use crate::ui_util::GuiContext;

pub type AudioPanelState = SharedAudioPanelState;

struct ConsoleAudioCommands<'a> {
    ctx: GuiContext<'a>,
}

impl AudioCommands for ConsoleAudioCommands<'_> {
    fn set_device(&mut self, device: Option<String>) {
        let _ = self
            .ctx
            .send_command(MetaCommand::UseInternalClocks(device));
    }

    fn set_filter_cutoff(&mut self, hz: f32) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::FilterCutoff(hz),
            )));
    }

    fn set_envelope_attack(&mut self, duration: Duration) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::EnvelopeAttack(duration),
            )));
    }

    fn set_envelope_release(&mut self, duration: Duration) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::EnvelopeRelease(duration),
            )));
    }

    fn set_output_smoothing(&mut self, duration: Duration) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::OutputSmoothing(duration),
            )));
    }

    fn set_gain(&mut self, gain_linear: f64) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::InputGain(gain_linear),
            )));
    }

    fn set_auto_trim_enabled(&mut self, enabled: bool) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::AutoTrimEnabled(enabled),
            )));
    }

    fn set_active_band(&mut self, band: u32) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::ActiveBand(band),
            )));
    }

    fn set_norm_floor_halflife(&mut self, halflife: Duration) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::NormFloorHalflife(halflife),
            )));
    }

    fn set_norm_ceiling_halflife(&mut self, halflife: Duration) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::NormCeilingHalflife(halflife),
            )));
    }

    fn set_norm_floor_mode(&mut self, mode: TrackingMode) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::NormFloorMode(mode),
            )));
    }

    fn set_norm_ceiling_mode(&mut self, mode: TrackingMode) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::NormCeilingMode(mode),
            )));
    }

    fn toggle_monitor(&mut self) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::ToggleMonitor));
    }

    fn reset_parameters(&mut self) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::ResetParameters));
    }

    fn list_devices(&mut self) -> Vec<String> {
        match AudioInput::devices() {
            Ok(d) => d,
            Err(e) => {
                self.ctx
                    .report_error(format_args!("Failed to list audio devices: {e}"));
                vec![]
            }
        }
    }

    fn report_error(&mut self, error: impl std::fmt::Display) {
        self.ctx.report_error(error);
    }
}

pub(crate) fn render_audio_panel(
    ui: &mut eframe::egui::Ui,
    ctx: GuiContext<'_>,
    state: &mut AudioPanelState,
    snapshot: &AudioSnapshot,
) {
    let mut commands = ConsoleAudioCommands { ctx };
    SharedAudioPanel {
        commands: &mut commands,
        state,
        snapshot,
    }
    .ui(ui);
}
