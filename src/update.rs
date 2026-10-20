//! Cobra Commander's release feed, and the version this build was released as.

use std::sync::Arc;

use app_update::{ReleaseFeed, ReleaseVersion, Updater};
use gui_common::update_panel::Spawner;

const FEED: ReleaseFeed = ReleaseFeed {
    owner: "generalelectrix",
    repo: "COBRA_COMMANDER",
    archive_asset: "CobraCommander.app.tar.gz",
    bundle_name: "Cobra Commander.app",
};

/// An updater for this build, or `None` when the build has no release version and so never
/// updates.
pub fn updater() -> Option<Updater> {
    let version = option_env!("COBRA_COMMANDER_VERSION")?.parse().ok()?;
    Some(updater_for(version))
}

/// An updater treating the running build as `version`.
pub fn updater_for(version: ReleaseVersion) -> Updater {
    Updater::new(FEED, version)
}

/// Runs update work on worker threads.
pub fn spawner() -> Spawner {
    Arc::new(|name, job| crate::worker::spawn(name, move |_shutdown| job()))
}
