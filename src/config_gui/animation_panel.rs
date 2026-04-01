use eframe::egui::{self, Color32};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints, Points};
use number::Phase;

use crate::gui_state::AnimationSnapshot;

#[derive(Default)]
pub struct VisualizerPanelState {
    preview: Vec<PlotPoint>,
    live: Vec<PlotPoint>,
    dots: Vec<PlotPoint>,
}

const NUM_WAVE_POINTS: usize = 1000;

impl VisualizerPanelState {
    /// Recompute plot data from the current animation snapshot.
    fn compute(&mut self, state: &AnimationSnapshot) {
        let phase_offset_per_fixture = if state.fixture_count == 0 {
            1.0
        } else {
            1.0 / state.fixture_count as f64
        };

        // Unit waveform (amplitude always 1).
        self.preview.clear();
        self.preview.extend((0..NUM_WAVE_POINTS).map(|i| {
            let phase = i as f64 / NUM_WAVE_POINTS as f64;
            let offset_index = (phase / phase_offset_per_fixture) as usize;
            let y = state.animation.get_unit_value(
                Phase::new(phase),
                offset_index,
                &state.clocks.clock_bank,
            );
            PlotPoint::new(phase, y)
        }));

        // Scaled waveform (applies audio envelope and animation scaling).
        self.live.clear();
        self.live.extend(self.preview.iter().map(|point| {
            PlotPoint::new(
                point.x,
                state.animation.scale_value(
                    &state.clocks.clock_bank,
                    state.clocks.audio_envelope,
                    point.y,
                ),
            )
        }));

        // Individual fixture dots.
        self.dots.clear();
        self.dots.extend((0..state.fixture_count).map(|i| {
            let phase = i as f64 * phase_offset_per_fixture;
            let y = state.animation.get_value(
                Phase::new(phase),
                i,
                &state.clocks.clock_bank,
                state.clocks.audio_envelope,
            );
            PlotPoint::new(phase, y)
        }));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &AnimationSnapshot) {
        self.compute(state);

        Plot::new("Animation")
            .default_x_bounds(0.0, 1.0)
            .default_y_bounds(-1.0, 1.0)
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new("Unit Waveform", PlotPoints::Borrowed(&self.preview))
                        .color(Color32::DARK_RED)
                        .width(2.0),
                );
                plot_ui.line(
                    Line::new("Scaled Waveform", PlotPoints::Borrowed(&self.live))
                        .color(Color32::WHITE)
                        .width(2.0),
                );
                plot_ui.points(
                    Points::new("Fixture Values", PlotPoints::Borrowed(&self.dots))
                        .color(Color32::CYAN)
                        .radius(5.0),
                );
            });

        // Continuously repaint while the visualizer is active.
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui_state::AnimationSnapshot;
    use egui_kittest::Harness;

    /// Run a few frames rather than `harness.run()`, because the visualizer
    /// intentionally requests continuous repaints.
    const SETTLE_STEPS: usize = 2;

    #[test]
    fn render_default_state() {
        let state = AnimationSnapshot::default();
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut VisualizerPanelState| {
                panel.ui(ui, &state);
            },
            VisualizerPanelState::default(),
        );
        harness.run_steps(SETTLE_STEPS);
        harness.snapshot("visualizer_panel_default");
    }

    #[test]
    fn render_with_fixtures() {
        let state = AnimationSnapshot {
            fixture_count: 4,
            ..Default::default()
        };
        let mut harness = Harness::new_ui_state(
            |ui, panel: &mut VisualizerPanelState| {
                panel.ui(ui, &state);
            },
            VisualizerPanelState::default(),
        );
        harness.run_steps(SETTLE_STEPS);
        harness.snapshot("visualizer_panel_with_fixtures");
    }

    #[test]
    fn compute_default_state() {
        let state = AnimationSnapshot::default();
        let mut panel = VisualizerPanelState::default();
        panel.compute(&state);

        assert_eq!(panel.preview.len(), NUM_WAVE_POINTS);
        assert_eq!(panel.live.len(), NUM_WAVE_POINTS);
        // Default fixture_count is 0, so no dots.
        assert_eq!(panel.dots.len(), 0);
    }

    #[test]
    fn compute_with_fixtures() {
        let state = AnimationSnapshot {
            fixture_count: 4,
            ..Default::default()
        };
        let mut panel = VisualizerPanelState::default();
        panel.compute(&state);

        assert_eq!(panel.preview.len(), NUM_WAVE_POINTS);
        assert_eq!(panel.live.len(), NUM_WAVE_POINTS);
        assert_eq!(panel.dots.len(), 4);

        // Dots should be evenly spaced across [0, 1).
        let phases: Vec<f64> = panel.dots.iter().map(|p| p.x).collect();
        assert!((phases[0] - 0.0).abs() < 1e-10);
        assert!((phases[1] - 0.25).abs() < 1e-10);
        assert!((phases[2] - 0.50).abs() < 1e-10);
        assert!((phases[3] - 0.75).abs() < 1e-10);
    }

    #[test]
    fn preview_x_values_span_unit_range() {
        let state = AnimationSnapshot::default();
        let mut panel = VisualizerPanelState::default();
        panel.compute(&state);

        assert!((panel.preview[0].x - 0.0).abs() < 1e-10);
        let last = panel.preview.last().unwrap();
        // Last point should be just under 1.0 (999/1000).
        assert!(last.x > 0.99 && last.x < 1.0);
    }

    #[test]
    fn live_has_same_x_as_preview() {
        let state = AnimationSnapshot {
            fixture_count: 2,
            ..Default::default()
        };
        let mut panel = VisualizerPanelState::default();
        panel.compute(&state);

        for (p, l) in panel.preview.iter().zip(panel.live.iter()) {
            assert!((p.x - l.x).abs() < 1e-10);
        }
    }

    #[test]
    fn recompute_clears_previous_data() {
        let mut panel = VisualizerPanelState::default();

        // First compute with 4 fixtures.
        panel.compute(&AnimationSnapshot {
            fixture_count: 4,
            ..Default::default()
        });
        assert_eq!(panel.dots.len(), 4);

        // Recompute with 2 fixtures — dots should shrink.
        panel.compute(&AnimationSnapshot {
            fixture_count: 2,
            ..Default::default()
        });
        assert_eq!(panel.dots.len(), 2);
    }
}
