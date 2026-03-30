use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::Result;

use super::model::Layout;
use super::parse::parse_touchosc_bytes;

macro_rules! include_group_template {
    ($map:expr, $name:expr) => {
        $map.insert(
            $name,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/touchosc/group_templates/",
                $name,
                ".touchosc"
            ))
            .as_slice(),
        );
    };
}

/// Raw bytes of each group template, keyed by fixture type name.
static GROUP_TEMPLATE_BYTES: LazyLock<HashMap<&'static str, &'static [u8]>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    include_group_template!(m, "Aquarius");
    include_group_template!(m, "Astera");
    include_group_template!(m, "Astroscan");
    include_group_template!(m, "Color");
    include_group_template!(m, "Colordynamic");
    include_group_template!(m, "Dimmer");
    include_group_template!(m, "FlashBang");
    include_group_template!(m, "FreedomFries");
    include_group_template!(m, "FreqStrobe");
    include_group_template!(m, "FusionRoll");
    include_group_template!(m, "H2O");
    include_group_template!(m, "Hypnotic");
    include_group_template!(m, "IWashLed");
    include_group_template!(m, "Lumitone");
    include_group_template!(m, "QuadPhase");
    include_group_template!(m, "Radiance");
    include_group_template!(m, "Relay");
    include_group_template!(m, "RotosphereQ3");
    include_group_template!(m, "RushWizard");
    include_group_template!(m, "SolarSystem");
    include_group_template!(m, "Starlight");
    include_group_template!(m, "TriPhase");
    include_group_template!(m, "Ufo");
    include_group_template!(m, "UvLedBrick");
    include_group_template!(m, "WizardExtreme");
    include_group_template!(m, "Wizlet");
    include_group_template!(m, "Wled");
    m
});

/// Raw bytes of the base template (always-on pages).
static BASE_TEMPLATE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/touchosc/base.touchosc"
));

/// Load the group template for a fixture type, if one exists.
pub fn load_group_template(fixture_type: &str) -> Option<Result<Layout>> {
    GROUP_TEMPLATE_BYTES
        .get(fixture_type)
        .map(|bytes| parse_touchosc_bytes(bytes))
}

/// Load the base template (always-on pages: channels, animation, master, etc.).
pub fn load_base_template() -> Result<Layout> {
    parse_touchosc_bytes(BASE_TEMPLATE_BYTES)
}
