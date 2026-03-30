/// A complete TouchOSC layout file.
///
/// All string fields are stored decoded (plain UTF-8).
/// Coordinates match the XML and editor: x is horizontal (left→right),
/// y is vertical (top→bottom), w is width, h is height.
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

#[derive(Debug, Clone, PartialEq)]
pub struct LabelStyle {
    pub t: String,
    pub c: String,
    pub s: String,
    pub o: String,
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
    /// Get the decoded OSC address, if present.
    pub fn osc_address(&self) -> Option<&str> {
        self.mid_attrs
            .iter()
            .find(|(k, _)| k == "osc_cs")
            .map(|(_, v)| v.as_str())
    }

    /// Get the OSC group prefix (first path component), if present.
    pub fn osc_group(&self) -> Option<&str> {
        self.osc_address()
            .and_then(|addr| addr.trim_start_matches('/').split('/').next())
    }

    /// Returns true if this is a label control (labelv or labelh).
    pub fn is_label(&self) -> bool {
        self.control_type == "labelv" || self.control_type == "labelh"
    }

    /// Check if this control's center falls within the given bounding box.
    pub fn center_within(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        let cx = self.x + self.w / 2;
        let cy = self.y + self.h / 2;
        cx >= x && cx <= x + w && cy >= y && cy <= y + h
    }
}

/// A MIDI binding on a control, stored as a raw attribute string
/// to preserve round-trip fidelity (TouchOSC uses `var ="x"` with
/// a space before `=` which is valid XML but non-standard).
#[derive(Debug, Clone, PartialEq)]
pub struct MidiBinding {
    pub raw_attrs: String,
}
