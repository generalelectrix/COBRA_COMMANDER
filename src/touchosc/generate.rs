use std::path::Path;

use anyhow::{Context, Result, anyhow};
use log::warn;

use super::model::*;
use super::regroup::set_group_name;
use super::serialize::write_touchosc;
use super::templates::{load_base_template, load_group_template};

/// Generate a complete TouchOSC layout file for a show.
///
/// Accepts an iterator of (group_name, fixture_type) pairs in patch order.
/// For each group, loads the fixture type's template and rewrites OSC addresses
/// to use the group name. Then appends the base pages (channels, animation,
/// master, audio, clocks, strobe). Writes the result to `output_path`.
///
/// Groups whose fixture type has no template are skipped with a warning.
pub fn generate_layout<'a>(
    groups: impl Iterator<Item = (&'a str, &'a str)>,
    output_path: &Path,
) -> Result<()> {
    let mut tabpages = Vec::new();

    for (group_name, fixture_type) in groups {
        let template = match load_group_template(fixture_type) {
            Some(Ok(layout)) => layout,
            Some(Err(e)) => {
                return Err(e).with_context(|| {
                    format!("failed to load template for fixture type '{fixture_type}'")
                });
            }
            None => {
                warn!(
                    "no TouchOSC template for fixture type '{fixture_type}' \
                     (group '{group_name}'), skipping"
                );
                continue;
            }
        };

        let mut page = template
            .tabpages
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("template for '{fixture_type}' has no pages"))?;

        set_group_name(&mut page, group_name);
        tabpages.push(page);
    }

    // Append base pages.
    let base = load_base_template().context("failed to load base template")?;
    tabpages.extend(base.tabpages);

    let layout = Layout {
        version: "17".to_string(),
        mode: "1".to_string(),
        orientation: Orientation::Vertical,
        tabpages,
    };

    write_touchosc(&layout, output_path)
        .with_context(|| format!("failed to write layout to {}", output_path.display()))
}
