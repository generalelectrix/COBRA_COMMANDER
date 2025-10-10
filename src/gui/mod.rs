use eframe::egui::Ui;

use crate::fixture::Patch;

struct PatchTable {}

impl PatchTable {
    pub fn ui(&mut self, patch: &Patch, ui: &mut Ui) {}
}
