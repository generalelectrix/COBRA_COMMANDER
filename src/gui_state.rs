use std::sync::{Arc, atomic::AtomicBool};

use arc_swap::ArcSwap;
use midi_harness::SlotStatus;
use tunnels::{animation::Animation, audio::AudioSnapshot, clock_server::SharedClockData};
use tunnels_lib::{notified::Notified, repaint::RepaintSignal};

use crate::config::FixtureGroupConfig;
use crate::osc::OscClientListener;

/// Snapshot of animation state for the visualizer panel.
#[derive(Default)]
pub struct AnimationSnapshot {
    pub animation: Animation,
    pub clocks: SharedClockData,
    pub fixture_count: usize,
}

/// Snapshot of the current patch configuration for the GUI.
#[derive(Clone, Debug, Default)]
pub struct PatchSnapshot {
    pub groups: Vec<FixtureGroupConfig>,
}

/// Port name from Display impl, used for both display and identity.
pub type PortName = String;

/// Snapshot of DMX port assignments for the GUI.
#[derive(Clone, Debug, Default)]
pub struct DmxPortStatus {
    /// One entry per universe, from the port's Display impl.
    pub ports: Vec<PortName>,
}

bitflags::bitflags! {
    /// GUI state domains that may need re-snapshotting after a control event.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GuiDirty: u8 {
        const CLEAN       = 0b0000_0000;
        const MIDI_SLOTS  = 0b0000_0001;
        const CLOCK_STATE = 0b0000_0010;
        const DMX_PORTS   = 0b0000_0100;
        const AUDIO       = 0b0000_1000;
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
    pub osc_clients: OscClientListener,
    /// Whether the visualizer tab is active — controls whether the Show
    /// snapshots animation state.
    pub visualizer_active: AtomicBool,
    pub animation_state: ArcSwap<AnimationSnapshot>,
    pub patch_snapshot: ArcSwap<PatchSnapshot>,
    pub dmx_port_status: ArcSwap<DmxPortStatus>,
    /// Whether the master strobe fader channel is mapped.
    pub master_strobe_fader_channel_mapped: AtomicBool,
    /// Snapshot of the current audio input state for the audio panel.
    pub audio_state: Notified<AudioSnapshot>,
}

impl GuiState {
    pub fn new(
        midi_slots: Vec<SlotStatus>,
        clock_status: ClockStatus,
        osc_listen_addr: String,
        osc_clients: OscClientListener,
        repaint: RepaintSignal,
    ) -> Self {
        Self {
            midi_slots: ArcSwap::from_pointee(midi_slots),
            clock_status: ArcSwap::from_pointee(clock_status),
            osc_listen_addr,
            osc_clients,
            visualizer_active: AtomicBool::new(false),
            animation_state: ArcSwap::from_pointee(AnimationSnapshot::default()),
            patch_snapshot: ArcSwap::from_pointee(PatchSnapshot::default()),
            dmx_port_status: ArcSwap::from_pointee(DmxPortStatus::default()),
            master_strobe_fader_channel_mapped: AtomicBool::new(false),
            audio_state: Notified::new(AudioSnapshot::default(), repaint),
        }
    }
}

/// Shared handle to the GUI state, held by both Show and GUI.
pub type SharedGuiState = Arc<GuiState>;
