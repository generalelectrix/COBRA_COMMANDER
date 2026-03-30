use std::collections::BTreeMap;

use super::model::*;

/// Split fixture-specific tabpages into one page per fixture group.
///
/// For each fixture tab, groups controls by their OSC group prefix.
/// Labels without OSC addresses are assigned to the group whose interactive
/// control they visually overlap (label-over-control pattern).
/// Each group gets its own page with cleaned-up layout.
pub fn split_fixture_pages(fixture_tabs: &[TabPage]) -> Vec<TabPage> {
    // Collect all controls across all tabs, grouped by fixture name.
    let mut by_group: BTreeMap<String, Vec<Control>> = BTreeMap::new();

    for tab in fixture_tabs {
        // Separate interactive controls (with OSC) from labels (without).
        let mut interactive: Vec<&Control> = Vec::new();
        let mut labels: Vec<&Control> = Vec::new();

        for ctrl in &tab.controls {
            if ctrl.osc_group().is_some() {
                interactive.push(ctrl);
            } else if ctrl.is_label() {
                labels.push(ctrl);
            }
            // Skip controls with no OSC and not labels.
        }

        // Group interactive controls by OSC group prefix.
        for ctrl in &interactive {
            let group = ctrl.osc_group().unwrap().to_string();
            by_group.entry(group).or_default().push((*ctrl).clone());
        }

        // Assign each label to the fixture group whose control it overlaps.
        for label in &labels {
            let mut assigned = false;
            for ctrl in &interactive {
                if label.center_within(ctrl.x, ctrl.y, ctrl.w, ctrl.h) {
                    let group = ctrl.osc_group().unwrap().to_string();
                    by_group.entry(group).or_default().push((*label).clone());
                    assigned = true;
                    break;
                }
            }
            if !assigned {
                // Label doesn't overlap any control. Try to find the nearest
                // interactive control and assign to its group.
                if let Some(nearest) = find_nearest_interactive(label, &interactive) {
                    let group = nearest.osc_group().unwrap().to_string();
                    by_group.entry(group).or_default().push((*label).clone());
                }
            }
        }
    }

    // Build one TabPage per fixture group with cleaned-up layout.
    by_group
        .into_iter()
        .map(|(group_name, controls)| {
            let mut page = TabPage {
                name: group_name.clone(),
                scalef: "0.0".to_string(),
                scalet: "1.0".to_string(),
                osc_cs: None,
                li: Some(LabelStyle {
                    t: group_name.clone(),
                    c: "gray".to_string(),
                    s: "14".to_string(),
                    o: "false".to_string(),
                    b: "false".to_string(),
                }),
                la: Some(LabelStyle {
                    t: group_name,
                    c: "gray".to_string(),
                    s: "14".to_string(),
                    o: "false".to_string(),
                    b: "false".to_string(),
                }),
                controls,
            };
            shift_to_top(&mut page);
            page
        })
        .collect()
}

/// Shift all controls toward the top of the page.
///
/// In the XML coordinate system, "top of page" in the landscape editor = high x.
/// We find the highest x+w among interactive controls, compute the gap between
/// that and the canvas top (~730), and shift all controls up by that amount.
fn shift_to_top(page: &mut TabPage) {
    const CANVAS_TOP: i32 = 730;
    const TOP_PADDING: i32 = 3;

    if page.controls.is_empty() {
        return;
    }

    // Use the max extent of ALL controls (including labels) so nothing
    // overflows past the canvas top.
    let max_x_extent = page
        .controls
        .iter()
        .map(|c| c.x + c.w)
        .max()
        .unwrap_or(0);

    let shift = (CANVAS_TOP - TOP_PADDING) - max_x_extent;
    if shift != 0 {
        for ctrl in &mut page.controls {
            ctrl.x += shift;
        }
    }
}

/// Find the nearest interactive control to a label, by center-to-center distance.
fn find_nearest_interactive<'a>(
    label: &Control,
    interactive: &[&'a Control],
) -> Option<&'a Control> {
    let lcx = label.x + label.w / 2;
    let lcy = label.y + label.h / 2;
    interactive
        .iter()
        .min_by_key(|ctrl| {
            let cx = ctrl.x + ctrl.w / 2;
            let cy = ctrl.y + ctrl.h / 2;
            let dx = (lcx - cx) as i64;
            let dy = (lcy - cy) as i64;
            dx * dx + dy * dy
        })
        .copied()
}
