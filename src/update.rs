//! Offering a newer release of Cobra Commander before it starts.

use std::time::Duration;

use app_update::{CheckError, ReleaseFeed, Updater};
use eframe::egui;
use gui_common::update_app::{self, UpdateChoice};

const FEED: ReleaseFeed = ReleaseFeed {
    owner: "generalelectrix",
    repo: "COBRA_COMMANDER",
    archive_asset: "CobraCommander.app.tar.gz",
    bundle_name: "Cobra Commander.app",
};

/// How long startup waits to hear of a newer release: ample for GitHub to answer, and short
/// enough that an offline show network barely delays the welcome screen.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Offer a newer release, if there is one, in a window styled by `setup`. Returns whether to
/// carry on starting. A build without a release version offers nothing.
pub fn offer(setup: impl FnOnce(&egui::Context) + 'static) -> bool {
    let Some(version) = option_env!("COBRA_COMMANDER_VERSION").and_then(|v| v.parse().ok()) else {
        return true;
    };
    let updater = Updater::new(FEED, version);
    let update = match updater.check(CHECK_TIMEOUT) {
        Ok(Some(update)) => update,
        Ok(None) => return true,
        Err(CheckError::BadRelease(e)) => {
            log::warn!("Update check: {e}");
            return true;
        }
        Err(e) => {
            log::info!("Update check failed: {e}");
            return true;
        }
    };
    match update_app::run("Cobra Commander", updater, update, setup) {
        Ok(choice) => choice == UpdateChoice::Continue,
        Err(e) => {
            log::error!("Couldn't show the update window: {e}");
            true
        }
    }
}
