use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::{FixtureGroupConfig, GroupId};
use crate::positioner::PositionerPresets;

/// File extension for show files (without the leading dot).
pub const EXTENSION: &str = "cobra";

/// Human-readable label used in file-dialog filters.
pub const FILTER_NAME: &str = "Cobra Show";

/// Default filename suggested when creating a new show.
pub const DEFAULT_FILE_NAME: &str = "show.cobra";

/// Extension used for the temporary file during atomic saves.
const TMP_EXTENSION: &str = "cobra.tmp";

/// The path to a show file on disk.
///
/// Wraps an `Arc<Path>` so the path can be cloned across thread boundaries with
/// only a refcount bump rather than a buffer reallocation. `Deref<Target = Path>`
/// lets it act as a `&Path` for any `Path`-shaped API (e.g. `show_file::save`).
#[derive(Clone)]
pub struct ShowPath(Arc<Path>);

impl ShowPath {
    pub fn new(path: impl Into<Arc<Path>>) -> Self {
        Self(path.into())
    }
}

impl std::ops::Deref for ShowPath {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

/// An immutable, cheap-clone handle to the show's patch configs.
///
/// Backed by `Arc<[T]>` so clones across thread boundaries cost a refcount
/// bump rather than reallocating the underlying slice. The configs themselves
/// are never mutated in place — repatch replaces the whole slice — so a
/// shared, immutable view is the natural shape.
pub type ShowPatchConfigs = Arc<[FixtureGroupConfig]>;

/// On-disk format for a Cobra Commander show file (`.cobra`).
#[derive(Debug, Serialize, Deserialize)]
pub struct ShowFile {
    pub patch: ShowPatchConfigs,
    #[serde(default)]
    pub positioners: HashMap<GroupId, PositionerPresets>,
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
    let tmp_path = path.with_extension(TMP_EXTENSION);
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
