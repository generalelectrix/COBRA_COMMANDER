use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use zip::write::SimpleFileOptions;

use super::model::*;

/// Write a Layout to a .touchosc file (ZIP containing index.xml).
pub fn write_touchosc(layout: &Layout, path: &Path) -> Result<()> {
    let xml = serialize_xml(layout);
    let file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("index.xml", options)?;
    zip.write_all(xml.as_bytes())?;
    zip.finish()?;
    Ok(())
}

/// Serialize a Layout to TouchOSC XML.
pub fn serialize_xml(layout: &Layout) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");

    let orientation_xml = match layout.orientation {
        Orientation::Horizontal => "horizontal",
        Orientation::Vertical => "vertical",
    };

    out.push_str(&format!(
        "<layout version=\"{}\" mode=\"{}\" orientation=\"{}\">",
        layout.version, layout.mode, orientation_xml
    ));

    for tp in &layout.tabpages {
        serialize_tabpage(&mut out, tp);
    }

    out.push_str("</layout>");
    out
}

fn serialize_tabpage(out: &mut String, tp: &TabPage) {
    out.push_str("<tabpage");
    write_attr(out, "name", &encode_b64(&tp.name));
    out.push_str(&format!(" scalef=\"{}\"", tp.scalef));
    out.push_str(&format!(" scalet=\"{}\"", tp.scalet));
    if let Some(ref osc) = tp.osc_cs {
        write_attr(out, "osc_cs", &encode_b64(osc));
    }
    if let Some(ref li) = tp.li {
        serialize_label_style(out, "li", li);
    }
    if let Some(ref la) = tp.la {
        serialize_label_style(out, "la", la);
    }
    // TouchOSC puts a trailing space before > on tabpage tags
    out.push_str(" >");

    for ctrl in &tp.controls {
        serialize_control(out, ctrl);
    }

    out.push_str("</tabpage>");
}

fn serialize_label_style(out: &mut String, prefix: &str, style: &LabelStyle) {
    write_attr(out, &format!("{prefix}_t"), &encode_b64(&style.t));
    write_attr(out, &format!("{prefix}_c"), &style.c);
    write_attr(out, &format!("{prefix}_s"), &style.s);
    write_attr(out, &format!("{prefix}_o"), &style.o);
    write_attr(out, &format!("{prefix}_b"), &style.b);
}

fn serialize_control(out: &mut String, ctrl: &Control) {
    out.push_str("<control");
    write_attr(out, "name", &encode_b64(&ctrl.name));
    out.push_str(&format!(" x=\"{}\"", ctrl.x));
    out.push_str(&format!(" y=\"{}\"", ctrl.y));
    out.push_str(&format!(" w=\"{}\"", ctrl.w));
    out.push_str(&format!(" h=\"{}\"", ctrl.h));
    write_attr(out, "color", &ctrl.color);

    // Mid attrs (between color and type): scalef, scalet, osc_cs
    for (k, v) in &ctrl.mid_attrs {
        if k == "osc_cs" {
            write_attr(out, k, &encode_b64(v));
        } else {
            write_attr(out, k, v);
        }
    }

    write_attr(out, "type", &ctrl.control_type);

    // Extra attrs (after type)
    for (k, v) in &ctrl.extra_attrs {
        if k == "text" {
            write_attr(out, k, &encode_b64(v));
        } else {
            write_attr(out, k, v);
        }
    }

    if ctrl.midi_bindings.is_empty() {
        // TouchOSC puts a trailing space before > on childless controls
        out.push_str(" ></control>");
    } else {
        // Also has trailing space before > even with children
        out.push_str(" >");
        for midi in &ctrl.midi_bindings {
            out.push_str("<midi ");
            out.push_str(&midi.raw_attrs);
            out.push_str("/>");
        }
        out.push_str("</control>");
    }
}

fn write_attr(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!(" {key}=\"{value}\""));
}

fn encode_b64(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    BASE64.encode(s.as_bytes())
}
