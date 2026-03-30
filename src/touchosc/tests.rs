use std::io::Read;
use std::path::Path;

use super::*;

/// Path to the touchosc templates directory.
fn touchosc_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("touchosc").leak()
}

/// Extract the raw XML from a .touchosc file for comparison.
fn extract_xml(path: &Path) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut index = archive.by_name("index.xml").unwrap();
    let mut xml = String::new();
    index.read_to_string(&mut xml).unwrap();
    xml
}

/// Parse a .touchosc file, serialize it back to XML, and assert the XML is identical.
fn assert_round_trip(filename: &str) {
    let path = touchosc_dir().join(filename);
    let original_xml = extract_xml(&path);
    let layout = parse_touchosc(&path).unwrap_or_else(|e| {
        panic!("failed to parse {filename}: {e}");
    });
    let reserialized_xml = serialize::serialize_xml(&layout);

    if original_xml != reserialized_xml {
        let orig_bytes = original_xml.as_bytes();
        let reser_bytes = reserialized_xml.as_bytes();
        let mut diff_pos = 0;
        for (i, (a, b)) in orig_bytes.iter().zip(reser_bytes.iter()).enumerate() {
            if a != b {
                diff_pos = i;
                break;
            }
        }
        if diff_pos == 0 && orig_bytes.len() != reser_bytes.len() {
            diff_pos = orig_bytes.len().min(reser_bytes.len());
        }

        let context_start = diff_pos.saturating_sub(40);
        let context_end_orig = (diff_pos + 40).min(original_xml.len());
        let context_end_reser = (diff_pos + 40).min(reserialized_xml.len());

        panic!(
            "round-trip mismatch for {filename} at byte {diff_pos}\n\
             original length: {}, reserialized length: {}\n\
             original[{}..{}]:     {:?}\n\
             reserialized[{}..{}]: {:?}",
            original_xml.len(),
            reserialized_xml.len(),
            context_start,
            context_end_orig,
            &original_xml[context_start..context_end_orig],
            context_start,
            context_end_reser,
            &reserialized_xml[context_start..context_end_reser],
        );
    }
}

#[test]
fn round_trip_master() {
    assert_round_trip("master.touchosc");
}

#[test]
fn round_trip_all_templates() {
    let dir = touchosc_dir();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "touchosc") {
            let filename = path.file_name().unwrap().to_str().unwrap();
            assert_round_trip(filename);
        }
    }
}

#[test]
fn parse_master_basic_structure() {
    let path = touchosc_dir().join("master.touchosc");
    let layout = parse_touchosc(&path).unwrap();

    assert_eq!(layout.version, "17");
    assert_eq!(layout.mode, "1");
    // XML says "vertical" — the editor shows it as landscape, but that's
    // the editor's business. We store the XML value as-is.
    assert_eq!(layout.orientation, Orientation::Vertical);
    assert_eq!(layout.tabpages.len(), 21);

    // First tab should be "H2O, Aquarius, Haze"
    assert_eq!(layout.tabpages[0].name, "H2O, Aquarius, Haze");

    // Animation tab (index 16)
    assert_eq!(layout.tabpages[16].name, "animation");

    // Master tab (index 17)
    assert_eq!(layout.tabpages[17].name, "master");
}

/// Write a test page with four labels at the corners to verify coordinate mapping.
///
/// In the XML/model coordinate system (orientation="vertical", displayed as landscape):
///   - x increases UPWARD in the editor (0 = bottom, ~730 = top)
///   - y increases RIGHTWARD in the editor (0 = left, ~1024 = right)
///   - w is the vertical extent (editor height of the control)
///   - h is the horizontal extent (editor width of the control)
///   - `labelv` renders text left-to-right in the editor
///   - `labelh` renders text vertically in the editor
#[test]
fn write_corner_test() {
    fn corner_label(name: &str, x: i32, y: i32) -> Control {
        Control {
            name: name.to_string(),
            x,
            y,
            w: 40,
            h: 150,
            color: "gray".to_string(),
            control_type: "labelv".to_string(),
            mid_attrs: Vec::new(),
            extra_attrs: vec![
                ("text".to_string(), name.to_string()),
                ("size".to_string(), "20".to_string()),
                ("background".to_string(), "true".to_string()),
                ("outline".to_string(), "false".to_string()),
            ],
            midi_bindings: Vec::new(),
        }
    }

    // XML canvas: x spans ~0..730, y spans ~0..1024.
    // In the editor (landscape): x goes up, y goes right.
    //   TopLeft:     high x (top), low y (left)
    //   TopRight:    high x (top), high y (right)
    //   BottomLeft:  low x (bottom), low y (left)
    //   BottomRight: low x (bottom), high y (right)
    let layout = Layout {
        version: "17".to_string(),
        mode: "1".to_string(),
        orientation: Orientation::Vertical,
        tabpages: vec![TabPage {
            name: "corners".to_string(),
            scalef: "0.0".to_string(),
            scalet: "1.0".to_string(),
            osc_cs: None,
            li: Some(LabelStyle {
                t: "corners".to_string(),
                c: "gray".to_string(),
                s: "14".to_string(),
                o: "false".to_string(),
                b: "false".to_string(),
            }),
            la: Some(LabelStyle {
                t: "corners".to_string(),
                c: "gray".to_string(),
                s: "14".to_string(),
                o: "false".to_string(),
                b: "false".to_string(),
            }),
            controls: vec![
                corner_label("TopLeft", 690, 0),
                corner_label("TopRight", 690, 874),
                corner_label("BottomLeft", 0, 0),
                corner_label("BottomRight", 0, 874),
            ],
        }],
    };

    let output_path = touchosc_dir().join("corner_test.touchosc");
    write_touchosc(&layout, &output_path).unwrap();

    let reparsed = parse_touchosc(&output_path).unwrap();
    assert_eq!(reparsed.tabpages.len(), 1);
    assert_eq!(reparsed.tabpages[0].controls.len(), 4);
}
