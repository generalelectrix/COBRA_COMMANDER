//! Snapshot structs for the console GUI.
//!
//! Written by the Show each frame, read by the GUI.

use std::net::SocketAddr;

use crate::fixture::patch::PatchOption;

/// Complete snapshot of show state for the console GUI.
#[derive(Clone)]
pub struct ConsoleState {
    /// Metadata about all registered fixture types (static, populated once).
    pub fixture_types: Vec<FixtureTypeMeta>,
    /// Current fixture groups in the patch.
    pub groups: Vec<GroupSummary>,
    /// Currently connected OSC clients.
    pub osc_clients: Vec<SocketAddr>,
    /// Currently connected MIDI input device names.
    pub midi_inputs: Vec<String>,
    /// Last error message, if any.
    pub last_error: Option<String>,
}

/// Metadata about a registered fixture type, derived from PATCHERS.
#[derive(Clone)]
pub struct FixtureTypeMeta {
    /// Name of the fixture type.
    pub name: String,
    /// Group-level options this fixture type accepts.
    pub group_options: Vec<(String, PatchOption)>,
    /// Patch-level options this fixture type accepts.
    pub patch_options: Vec<(String, PatchOption)>,
}

/// Summary of a fixture group currently in the patch.
#[derive(Clone)]
pub struct GroupSummary {
    /// Group key (fixture type name or explicit group name).
    pub key: String,
    /// The fixture type for this group.
    pub fixture_type: String,
    /// Whether this group is assigned to a channel.
    pub channel: bool,
    /// Whether this group has a color organ.
    pub color_organ: bool,
    /// Group-level option values as key/value strings.
    pub options: Vec<(String, String)>,
    /// Individual patches in this group.
    pub patches: Vec<PatchSummary>,
}

/// Summary of a single fixture patch within a group.
#[derive(Clone)]
pub struct PatchSummary {
    /// 1-based DMX address, None for non-DMX fixtures.
    pub addr: Option<u32>,
    /// Universe index.
    pub universe: usize,
    /// Whether this patch is mirrored.
    pub mirror: bool,
    /// Number of DMX channels used.
    pub channel_count: usize,
    /// Patch-level option values as key/value strings.
    pub options: Vec<(String, String)>,
}
