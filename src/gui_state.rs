use std::net::IpAddr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize},
};

use arc_swap::ArcSwap;
use midi_harness::SlotStatus;
use tunnels::{animation::Animation, audio::AudioSnapshot, clock_server::SharedClockData};
use tunnels_lib::{notified::Notified, repaint::RepaintSignal};

use crate::dmx::{DmxBuffer, UniverseIdx};
use crate::osc::OscClientId;
use crate::show_file::ShowPatchConfigs;

/// Snapshot of animation state for the visualizer panel.
#[derive(Default)]
pub struct AnimationSnapshot {
    pub animation: Animation,
    pub clocks: SharedClockData,
    pub fixture_count: usize,
}

/// Snapshot of the patch configuration.
#[derive(Clone, Debug, Default)]
pub struct PatchSnapshot {
    pub groups: ShowPatchConfigs,
}

/// Port name from Display impl, used for both display and identity.
pub type PortName = String;

/// Per-universe DMX port info exposed to the GUI.
#[derive(Clone, Debug)]
pub struct DmxPortInfo {
    /// Display string from the port (used as identity and the row label).
    pub name: PortName,
    /// Current output framerate in FPS, mirroring `DmxPort::get_framerate()`.
    /// `None` when the port does not support framerate control.
    pub framerate: Option<u8>,
}

/// Snapshot of DMX port assignments for the GUI.
#[derive(Clone, Debug, Default)]
pub struct DmxPortStatus {
    /// One entry per universe.
    pub ports: Vec<DmxPortInfo>,
}

/// Sentinel `dmx_debug_watch` value meaning "no debug window is watching".
pub const DMX_DEBUG_NOT_WATCHING: usize = usize::MAX;

/// Snapshot of the live DMX output buffer for one universe, for the output
/// debug window. Carries its universe index so the GUI can discard stale data
/// left over from a previous selection while a switch is in flight.
#[derive(Clone, Debug)]
pub struct DmxDebugSnapshot {
    pub universe: UniverseIdx,
    pub values: DmxBuffer,
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
        const OSC_CLIENTS = 0b0001_0000;
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
    pub midi_slots: Notified<Vec<SlotStatus>>,
    pub clock_status: ArcSwap<ClockStatus>,
    /// The host's primary local IP, or `None` when none can be resolved.
    /// Refreshed as network interfaces change.
    pub osc_local_ip: Notified<Option<IpAddr>>,
    pub osc_clients: Notified<Vec<OscClientId>>,
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
    /// Universe the DMX output debug window is watching, written by the GUI and
    /// read by the Show each loop. `DMX_DEBUG_NOT_WATCHING` when no window is open
    /// — gates whether the Show snapshots output at all.
    pub dmx_debug_watch: AtomicUsize,
    /// Snapshot of the live DMX output buffer for the watched universe, pushed by
    /// the Show at ~4fps. `None` until the first snapshot for a selection arrives.
    pub dmx_debug: Notified<Option<DmxDebugSnapshot>>,
}

impl GuiState {
    /// `repaint` wakes the root viewport (where all the main-window panels
    /// live). `dmx_debug_repaint` is a separate signal that must also wake the
    /// DMX debug viewport — the debug window is a distinct deferred viewport, so
    /// a root-only repaint would never re-render it (see `dmx_debug`).
    pub fn new(
        midi_slots: Vec<SlotStatus>,
        clock_status: ClockStatus,
        osc_local_ip: Option<IpAddr>,
        repaint: RepaintSignal,
        dmx_debug_repaint: RepaintSignal,
    ) -> Self {
        Self {
            midi_slots: Notified::new(midi_slots, repaint.clone()),
            clock_status: ArcSwap::from_pointee(clock_status),
            osc_local_ip: Notified::new(osc_local_ip, repaint.clone()),
            osc_clients: Notified::new(Vec::new(), repaint.clone()),
            visualizer_active: AtomicBool::new(false),
            animation_state: ArcSwap::from_pointee(AnimationSnapshot::default()),
            patch_snapshot: ArcSwap::from_pointee(PatchSnapshot::default()),
            dmx_port_status: ArcSwap::from_pointee(DmxPortStatus::default()),
            master_strobe_fader_channel_mapped: AtomicBool::new(false),
            audio_state: Notified::new(AudioSnapshot::default(), repaint),
            dmx_debug_watch: AtomicUsize::new(DMX_DEBUG_NOT_WATCHING),
            dmx_debug: Notified::new(None, dmx_debug_repaint),
        }
    }
}

/// Shared handle to the GUI state, held by both Show and GUI.
pub type SharedGuiState = Arc<GuiState>;
