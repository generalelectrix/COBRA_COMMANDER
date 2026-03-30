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
