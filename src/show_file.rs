use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::FixtureGroupConfig;

/// On-disk format for a Cobra Commander show file (`.cobra`).
///
/// Wraps the patch data so we can add sibling fields in the future.
#[derive(Serialize, Deserialize)]
pub struct ShowFile {
    pub patch: Vec<FixtureGroupConfig>,
}

/// Load a show file from disk.
pub fn load(path: &Path) -> Result<ShowFile> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("unable to read show file \"{}\"", path.display()))?;
    serde_yaml::from_str(&contents)
        .with_context(|| format!("unable to parse show file \"{}\"", path.display()))
}

/// Save a show file to disk atomically.
///
/// Writes to a temporary file first, then renames to the target path.
/// This ensures the target file is never left in a partially-written state.
pub fn save(path: &Path, file: &ShowFile) -> Result<()> {
    let tmp_path = path.with_extension("cobra.tmp");
    let contents = serde_yaml::to_string(file).context("unable to serialize show file")?;
    fs::write(&tmp_path, contents)
        .with_context(|| format!("unable to write temporary file \"{}\"", tmp_path.display()))?;
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e).with_context(|| {
            format!(
                "unable to rename \"{}\" to \"{}\"",
                tmp_path.display(),
                path.display()
            )
        });
    }
    Ok(())
}
