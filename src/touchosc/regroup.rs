use super::model::TabPage;

/// Rewrite all OSC addresses in a template page, replacing the current
/// group prefix with the given group name. Also updates the page name and
/// label styling text to match the group name.
///
/// For example, if the template page is named `"Color"` with addresses like
/// `/Color/Hue` and the group name is `"Front"`, the addresses become `/Front/Hue`.
pub fn set_group_name(page: &mut TabPage, group_name: &str) {
    let fixture_type = page.name.clone();
    let old_prefix = format!("/{fixture_type}/");

    // Update OSC addresses on all controls.
    for ctrl in &mut page.controls {
        for (key, value) in &mut ctrl.mid_attrs {
            if key == "osc_cs" {
                if let Some(suffix) = value.strip_prefix(&old_prefix) {
                    *value = format!("/{group_name}/{suffix}");
                }
            }
        }
    }

    // Update page name.
    page.name = group_name.to_string();

    // Update label styling text (li_t, la_t).
    if let Some(ref mut li) = page.li {
        if li.t == fixture_type {
            li.t = group_name.to_string();
        }
    }
    if let Some(ref mut la) = page.la {
        if la.t == fixture_type {
            la.t = group_name.to_string();
        }
    }

    // Update tabpage osc_cs if it references the fixture type.
    if let Some(ref mut osc) = page.osc_cs {
        if let Some(suffix) = osc.strip_prefix(&old_prefix) {
            *osc = format!("/{group_name}/{suffix}");
        } else if *osc == format!("/{fixture_type}") {
            *osc = format!("/{group_name}");
        }
    }
}
