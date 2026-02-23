//! Commands sent from the console GUI to the Show.

use std::net::SocketAddr;

use crate::config::FixtureGroupConfig;

/// Commands the console GUI can send to the running show.
pub enum ConsoleCommand {
    /// Apply a new patch configuration.
    Repatch(Vec<FixtureGroupConfig>),
    /// Add a new OSC client.
    AddOscClient(SocketAddr),
    /// Remove an OSC client.
    RemoveOscClient(SocketAddr),
    /// Trigger a MIDI device rescan.
    RescanMidi,
}
