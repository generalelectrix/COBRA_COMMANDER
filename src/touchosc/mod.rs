mod extract;
mod model;
mod parse;
mod serialize;

pub use model::*;
pub use parse::parse_touchosc;
pub use serialize::write_touchosc;

use std::path::{Path, PathBuf};

/// Return the path to the group template for the given fixture type name.
pub fn group_template_path(fixture_type: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("touchosc")
        .join("group_templates")
        .join(format!("{fixture_type}.touchosc"))
}

/// Load the group template for the given fixture type name, if one exists.
pub fn load_group_template(fixture_type: &str) -> Option<anyhow::Result<Layout>> {
    let path = group_template_path(fixture_type);
    if path.exists() {
        Some(parse_touchosc(&path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
