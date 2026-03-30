use anyhow::Result;
use linkme::distributed_slice;

use super::model::Layout;
use super::parse::parse_touchosc_bytes;

/// A compiled-in TouchOSC template for a fixture type.
pub struct TemplateEntry {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

/// Distributed registry of fixture group templates.
///
/// Fixture types register their templates here via the `PatchFixture` derive
/// macro (default) or `register_patcher!`. Use `#[no_touchosc_template]` on
/// the fixture struct to opt out.
#[distributed_slice]
pub static TEMPLATES: [TemplateEntry];

/// Raw bytes of the base template (always-on pages).
static BASE_TEMPLATE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/touchosc/base.touchosc"
));

/// Load the group template for a fixture type, if one exists.
pub fn load_group_template(fixture_type: &str) -> Option<Result<Layout>> {
    TEMPLATES
        .iter()
        .find(|entry| entry.name == fixture_type)
        .map(|entry| parse_touchosc_bytes(entry.bytes))
}

/// Load the base template (always-on pages: channels, animation, master, etc.).
pub fn load_base_template() -> Result<Layout> {
    parse_touchosc_bytes(BASE_TEMPLATE_BYTES)
}
