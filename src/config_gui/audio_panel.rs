use std::time::Duration;

use eframe::egui;

use gui_common::audio_panel::{
    AudioCommands, AudioPanel as SharedAudioPanel, AudioPanelState as SharedAudioPanelState,
    AudioSnapshot,
};
use tunnels::audio::time::HalfLife;
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

    fn set_norm_floor_halflife(&mut self, halflife: HalfLife) {
        let _ = self
            .ctx
            .send_command(MetaCommand::AudioControl(ControlMessage::Set(
                StateChange::NormFloorHalflife(halflife),
            )));
    }

    fn set_norm_ceiling_halflife(&mut self, halflife: HalfLife) {
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

/// The selector for the audio role the show follows, on its own.
pub(crate) fn render_follow_selector(ui: &mut egui::Ui, ctx: GuiContext<'_>, active_role: Role) {
    ui.horizontal(|ui| {
        ui.label("Follow:");
        if let Some(role) = gui_common::audio_panel::follow_selector(ui, active_role) {
            ConsoleAudioCommands { ctx }.set_active_role(role);
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::mock::recording_client;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use gui_common::MessageModal;

    /// The follow selector offers every role and asks the show to follow the
    /// one picked.
    #[test]
    fn follow_selector_sets_the_active_role() {
        let (client, recorded) = recording_client();
        let mut modal = MessageModal::default();
        let mut harness = Harness::new_ui(|ui| {
            render_follow_selector(
                ui,
                GuiContext {
                    modal: &mut modal,
                    client: &client,
                },
                Role::Bass,
            );
        });
        harness.run();

        harness.get_by_value("Bass").click();
        harness.run();
        for role in Role::ALL {
            assert!(
                harness.query_all_by_label(role.label()).next().is_some(),
                "{} is offered",
                role.label()
            );
        }
        harness.get_by_label("Hats").click();
        harness.run();

        let recorded = recorded.lock().expect("recording log poisoned");
        assert_eq!(*recorded, ["SetActiveRole(Hats)"]);
    }
}
