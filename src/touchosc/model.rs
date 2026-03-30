/// A complete TouchOSC layout file.
///
/// All string fields are stored decoded (plain UTF-8).
/// Coordinates are stored as raw XML portrait values — see CLAUDE.md
/// for how these map to the landscape editor view.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub version: String,
    pub mode: String,
    pub orientation: Orientation,
    pub tabpages: Vec<TabPage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabPage {
    pub name: String,
    pub scalef: String,
    pub scalet: String,
    /// Optional base OSC address for the page.
    pub osc_cs: Option<String>,
    /// Label indicator styling.
    pub li: Option<LabelStyle>,
    /// Label styling.
    pub la: Option<LabelStyle>,
    pub controls: Vec<Control>,
}

impl TabPage {
    /// Rewrite all OSC addresses, replacing the current group prefix with the
    /// given group name. Also updates the page name and label styling text.
    ///
    /// For example, if the page is named `"Color"` with addresses like
    /// `/Color/Hue` and the group name is `"Front"`, the addresses become
    /// `/Front/Hue`.
    pub fn set_group_name(&mut self, group_name: &str) {
        let fixture_type = self.name.clone();
        let old_prefix = format!("/{fixture_type}/");

        for ctrl in &mut self.controls {
            for (key, value) in &mut ctrl.mid_attrs {
                if key == "osc_cs" {
                    if let Some(suffix) = value.strip_prefix(&old_prefix) {
                        *value = format!("/{group_name}/{suffix}");
                    }
                }
            }
        }

        self.name = group_name.to_string();

        if let Some(ref mut li) = self.li {
            if li.t == fixture_type {
                li.t = group_name.to_string();
            }
        }
        if let Some(ref mut la) = self.la {
            if la.t == fixture_type {
                la.t = group_name.to_string();
            }
        }

        if let Some(ref mut osc) = self.osc_cs {
            if let Some(suffix) = osc.strip_prefix(&old_prefix) {
                *osc = format!("/{group_name}/{suffix}");
            } else if *osc == format!("/{fixture_type}") {
                *osc = format!("/{group_name}");
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelStyle {
    /// Text content.
    pub t: String,
    /// Color name.
    pub c: String,
    /// Font size.
    pub s: String,
    /// Outline enabled.
    pub o: String,
    /// Bold enabled.
    pub b: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: String,
    pub control_type: String,
    /// Extra attributes in the order they appear after `type`, excluding
    /// the common prefix (name, x, y, w, h, color) and any attrs that appear
    /// between color and type (scalef, scalet, osc_cs).
    /// This preserves round-trip fidelity for type-specific attrs we don't
    /// need to model individually yet.
    pub extra_attrs: Vec<(String, String)>,
    /// Attributes that appear between color and type: scalef, scalet, osc_cs.
    /// Stored in order for round-trip fidelity.
    pub mid_attrs: Vec<(String, String)>,
    pub midi_bindings: Vec<MidiBinding>,
}

impl Control {
    /// Returns true if this is a label control (labelv or labelh).
    pub fn is_label(&self) -> bool {
        self.control_type == "labelv" || self.control_type == "labelh"
    }

    /// Get the decoded OSC address, if present.
    pub fn osc_address(&self) -> Option<&str> {
        self.mid_attrs
            .iter()
            .find(|(k, _)| k == "osc_cs")
            .map(|(_, v)| v.as_str())
    }
}

/// A MIDI binding on a control, stored as a raw attribute string
/// to preserve round-trip fidelity (TouchOSC uses `var ="x"` with
/// a space before `=` which is valid XML but non-standard).
#[derive(Debug, Clone, PartialEq)]
pub struct MidiBinding {
    pub raw_attrs: String,
}
