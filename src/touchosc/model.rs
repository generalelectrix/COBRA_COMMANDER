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
    #[expect(unused)]
    pub fn is_label(&self) -> bool {
        self.control_type == "labelv" || self.control_type == "labelh"
    }

    /// Get the decoded OSC address, if present.
    #[expect(unused)]
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

/// Raw bytes of a `.touchosc` file (a ZIP archive containing `index.xml`).
///
/// Wraps either owned or borrowed bytes. Use `From<Vec<u8>>` or
/// `From<&'static [u8]>` to construct, or the struct literal for statics.
#[derive(Debug, Clone)]
pub struct TouchOscZip(pub std::borrow::Cow<'static, [u8]>);

impl From<Vec<u8>> for TouchOscZip {
    fn from(v: Vec<u8>) -> Self {
        Self(std::borrow::Cow::Owned(v))
    }
}

impl From<&'static [u8]> for TouchOscZip {
    fn from(s: &'static [u8]) -> Self {
        Self(std::borrow::Cow::Borrowed(s))
    }
}

impl TouchOscZip {
    /// Extract the raw XML from this ZIP archive.
    pub fn extract_xml(&self) -> anyhow::Result<TouchOscXml> {
        use anyhow::Context;
        use std::io::{Cursor, Read};
        let mut archive = zip::ZipArchive::new(Cursor::new(self.0.as_ref()))
            .context("failed to read ZIP archive")?;
        let mut index = archive
            .by_name("index.xml")
            .context("ZIP archive missing index.xml")?;
        let mut xml = Vec::new();
        index
            .read_to_end(&mut xml)
            .context("failed to read index.xml from ZIP")?;
        Ok(TouchOscXml(xml))
    }

    /// Parse this ZIP into a Layout.
    pub fn parse(&self) -> anyhow::Result<Layout> {
        use anyhow::Context;
        let xml = self.extract_xml()?;
        let xml = String::from_utf8(xml.0).context("index.xml is not valid UTF-8")?;
        super::parse::parse_xml(&xml)
    }
}

/// Raw XML content extracted from a `.touchosc` file.
#[derive(Debug, Clone)]
pub struct TouchOscXml(pub Vec<u8>);

impl Layout {
    /// Serialize this layout to in-memory .touchosc ZIP bytes.
    pub fn to_zip(&self) -> anyhow::Result<TouchOscZip> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let xml = super::serialize::serialize_xml(self);
        let mut buf = std::io::Cursor::new(Vec::new());
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("index.xml", options)?;
            zip.write_all(xml.as_bytes())?;
            zip.finish()?;
        }
        Ok(TouchOscZip::from(buf.into_inner()))
    }

    /// Write this layout to a .touchosc file on disk.
    pub fn write(&self, path: &std::path::Path) -> anyhow::Result<()> {
        use anyhow::Context;
        let zip = self.to_zip()?;
        std::fs::write(path, &zip.0).with_context(|| format!("failed to write {}", path.display()))
    }
}
