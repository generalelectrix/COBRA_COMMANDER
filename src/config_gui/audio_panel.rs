use std::time::Duration;

use gui_common::audio_panel::{
    AudioCommands, AudioPanel as SharedAudioPanel, AudioPanelState as SharedAudioPanelState,
    AudioSnapshot,
};
use tunnels::audio::{AudioInput, ControlMessage, Role, StateChange};

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

    fn set_active_role(&mut self, role: Role) {
        let _ = self.ctx.send_command(MetaCommand::SetActiveRole(role));
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
}

pub(crate) fn render_audio_panel(
    ui: &mut eframe::egui::Ui,
    ctx: GuiContext<'_>,
    state: &mut AudioPanelState,
    snapshot: &AudioSnapshot,
    active_role: Role,
) {
    let mut commands = ConsoleAudioCommands { ctx };
    SharedAudioPanel {
        commands: &mut commands,
        state,
        snapshot,
        active_role,
    }
    .ui(ui);
}
