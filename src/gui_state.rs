use std::sync::Arc;

use arc_swap::ArcSwap;
use midi_harness::SlotStatus;

bitflags::bitflags! {
    /// GUI state domains that may need re-snapshotting after a control event.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GuiDirty: u8 {
        const CLEAN      = 0b0000_0000;
        const MIDI_SLOTS = 0b0000_0001;
    }
}

/// Lock-free shared state from Show → GUI.
/// Each field is independently and atomically swappable.
pub struct GuiState {
    pub midi_slots: ArcSwap<Vec<SlotStatus>>,
}

impl GuiState {
    pub fn new() -> Self {
        Self {
            midi_slots: ArcSwap::from_pointee(Vec::new()),
        }
    }
}

/// Shared handle to the GUI state, held by both Show and GUI.
pub type SharedGuiState = Arc<GuiState>;
