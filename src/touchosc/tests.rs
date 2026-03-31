use std::io::Read;
use std::path::Path;

use super::*;

/// Path to the touchosc templates directory.
fn touchosc_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("touchosc")
        .leak()
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
fn generate_layout_from_patch() {
    let groups = vec![
        GroupEntry {
            group_name: "Front",
            fixture_type: "Color",
        },
        GroupEntry {
            group_name: "Top",
            fixture_type: "Color",
        },
        GroupEntry {
            group_name: "TriPhase",
            fixture_type: "TriPhase",
        },
        GroupEntry {
            group_name: "Starlight",
            fixture_type: "Starlight",
        },
    ];

    let output_path = touchosc_dir().join("test.touchosc");
    generate_layout(groups.into_iter(), &output_path).unwrap();

    let layout = parse_touchosc(&output_path).unwrap();

    // 4 fixture pages + 6 base pages = 10 total.
    assert_eq!(layout.tabpages.len(), 10);

    // Fixture pages come first in patch order.
    assert_eq!(layout.tabpages[0].name, "Front");
    assert_eq!(layout.tabpages[1].name, "Top");
    assert_eq!(layout.tabpages[2].name, "TriPhase");
    assert_eq!(layout.tabpages[3].name, "Starlight");

    // Base pages follow.
    assert_eq!(layout.tabpages[4].name, "channels");

    // Verify address rewriting on renamed groups.
    let front_addrs: Vec<_> = layout.tabpages[0]
        .controls
        .iter()
        .filter_map(|c| c.osc_address())
        .collect();
    assert!(
        front_addrs.iter().all(|a| a.starts_with("/Front/")),
        "Front page has unrewritten addresses: {front_addrs:?}"
    );

    // TriPhase should keep its original addresses since group == fixture type.
    let tri_addrs: Vec<_> = layout.tabpages[2]
        .controls
        .iter()
        .filter_map(|c| c.osc_address())
        .collect();
    assert!(
        tri_addrs.iter().all(|a| a.starts_with("/TriPhase/")),
        "TriPhase addresses changed unexpectedly: {tri_addrs:?}"
    );

    std::fs::remove_file(&output_path).ok();
}

/// Applying set_group_name with the same name as the fixture type should
/// produce byte-identical XML output.
#[test]
fn set_group_name_identity() {
    let templates_dir = touchosc_dir().join("group_templates");
    for entry in std::fs::read_dir(&templates_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.extension().is_some_and(|ext| ext == "touchosc") {
            continue;
        }
        let fixture_type = path.file_stem().unwrap().to_str().unwrap();
        let layout = parse_touchosc(&path).unwrap();
        let original_xml = extract_xml(&path);

        let mut page = layout.tabpages[0].clone();
        page.set_group_name(fixture_type);

        let mut modified_layout = layout.clone();
        modified_layout.tabpages[0] = page;
        let modified_xml = serialize::serialize_xml(&modified_layout);

        assert_eq!(
            original_xml, modified_xml,
            "identity rewrite changed XML for {fixture_type}"
        );
    }
}

#[test]
fn set_group_name_renames_addresses() {
    let layout = load_group_template("Color").unwrap().unwrap();
    let mut page = layout.tabpages[0].clone();

    // Verify original addresses.
    let addrs: Vec<_> = page
        .controls
        .iter()
        .filter_map(|c| c.osc_address().map(String::from))
        .collect();
    assert!(addrs.iter().all(|a| a.starts_with("/Color/")));

    // Rename to "FrontWash".
    page.set_group_name("FrontWash");

    assert_eq!(page.name, "FrontWash");
    for ctrl in &page.controls {
        if let Some(addr) = ctrl.osc_address() {
            assert!(
                addr.starts_with("/FrontWash/"),
                "address not renamed: {addr}"
            );
        }
    }
}

#[test]
fn layout_server_serves_xml() {
    use super::templates::BASE_TEMPLATE;
    let zip = &BASE_TEMPLATE;
    let expected_xml = zip.extract_xml().unwrap();

    let server = serve::LayoutServer::start("TestLayout".to_string(), &zip).unwrap();

    // Give the server thread a moment to start accepting.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Make a raw HTTP request and check the response.
    let mut stream = std::net::TcpStream::connect("127.0.0.1:9658").unwrap();
    std::io::Write::write_all(&mut stream, b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();

    let header_end = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("no header/body separator");
    let headers = std::str::from_utf8(&response[..header_end]).unwrap();
    let body = &response[header_end + 4..];

    // Verify headers.
    assert!(
        headers.contains("Content-Type: application/touchosc"),
        "missing content type header: {headers}"
    );
    assert!(
        headers.contains("Content-Disposition: attachment; filename=\"TestLayout.touchosc\""),
        "missing or wrong content disposition header: {headers}"
    );

    // Verify body is the raw XML, not the ZIP.
    assert!(
        body.starts_with(b"<?xml"),
        "body should be raw XML, got: {:?}",
        &body[..20.min(body.len())]
    );
    assert_eq!(
        body,
        expected_xml.0.as_slice(),
        "body doesn't match expected XML"
    );

    server.stop().unwrap();
}
