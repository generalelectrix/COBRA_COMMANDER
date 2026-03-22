use std::sync::Arc;

use arc_swap::ArcSwap;
use midi_harness::SlotStatus;

use crate::osc::OscClientReader;

bitflags::bitflags! {
    /// GUI state domains that may need re-snapshotting after a control event.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GuiDirty: u8 {
        const CLEAN       = 0b0000_0000;
        const MIDI_SLOTS  = 0b0000_0001;
        const CLOCK_STATE = 0b0000_0010;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClockStatus {
    Internal { audio_device: String },
    Remote { provider: String },
}

/// Lock-free shared state from Show → GUI.
/// Each field is independently and atomically swappable.
pub struct GuiState {
    pub midi_slots: ArcSwap<Vec<SlotStatus>>,
    pub clock_status: ArcSwap<ClockStatus>,
    pub osc_listen_addr: String,
    pub osc_clients: OscClientReader,
}

impl GuiState {
    pub fn new(
        midi_slots: Vec<SlotStatus>,
        clock_status: ClockStatus,
        osc_listen_addr: String,
        osc_clients: OscClientReader,
    ) -> Self {
        Self {
            midi_slots: ArcSwap::from_pointee(midi_slots),
            clock_status: ArcSwap::from_pointee(clock_status),
            osc_listen_addr,
            osc_clients,
        }
    }
}

/// Shared handle to the GUI state, held by both Show and GUI.
pub type SharedGuiState = Arc<GuiState>;
