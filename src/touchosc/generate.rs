use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::model::*;
use super::templates::{load_base_template, load_group_template};

/// A fixture group entry for layout generation.
pub struct GroupEntry<'a> {
    /// The group name used in OSC addresses (e.g. "Front", "Top").
    pub group_name: &'a str,
    /// The fixture type name used to look up the template (e.g. "Color", "TriPhase").
    pub fixture_type: &'a str,
}

/// Assemble a complete TouchOSC layout for a show.
///
/// For each group, loads the fixture type's template and rewrites OSC addresses
/// to use the group name. Then appends the base pages (channels, animation,
/// master, audio, clocks, strobe).
///
/// Groups whose fixture type has no template are skipped with a warning.
pub fn assemble_layout<'a>(groups: impl Iterator<Item = GroupEntry<'a>>) -> Result<Layout> {
    let mut tabpages = Vec::new();

    for GroupEntry {
        group_name,
        fixture_type,
    } in groups
    {
        let template = match load_group_template(fixture_type) {
            Some(Ok(layout)) => layout,
            Some(Err(e)) => {
                return Err(e).with_context(|| {
                    format!("failed to load template for fixture type '{fixture_type}'")
                });
            }
            None => {
                continue;
            }
        };

        let mut page = template
            .tabpages
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("template for '{fixture_type}' has no pages"))?;

        page.set_group_name(group_name);
        // Suppress the page tab's own OSC message; without this TouchOSC
        // auto-sends /{group_name} whenever the tab is selected.
        page.osc_cs = Some("/ignore".to_string());
        tabpages.push(page);
    }

    // Append base pages.
    let base = load_base_template().context("failed to load base template")?;
    tabpages.extend(base.tabpages);

    Ok(Layout {
        version: "17".to_string(),
        mode: "1".to_string(),
        orientation: Orientation::Vertical,
        tabpages,
    })
}

/// Generate a complete TouchOSC layout file for a show and write it to disk.
pub fn generate_layout<'a>(
    groups: impl Iterator<Item = GroupEntry<'a>>,
    output_path: &Path,
) -> Result<()> {
    let layout = assemble_layout(groups)?;
    layout
        .write(output_path)
        .with_context(|| format!("failed to write layout to {}", output_path.display()))
}
