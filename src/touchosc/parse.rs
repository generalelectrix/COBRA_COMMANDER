use std::io::{Cursor, Read};
use std::path::Path;

use super::model::*;
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use quick_xml::Reader;
use quick_xml::events::Event;

/// Parse a .touchosc file from disk.
pub fn parse_touchosc(path: &Path) -> Result<Layout> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    parse_touchosc_bytes(&bytes)
}

/// Parse a .touchosc file from in-memory bytes (ZIP archive).
pub fn parse_touchosc_bytes(bytes: &[u8]) -> Result<Layout> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).context("failed to read ZIP archive")?;
    let mut index = archive
        .by_name("index.xml")
        .context("ZIP archive missing index.xml")?;
    let mut xml = String::new();
    index
        .read_to_string(&mut xml)
        .context("failed to read index.xml from ZIP")?;
    parse_xml(&xml)
}

/// Parse the raw XML content of a TouchOSC layout.
pub fn parse_xml(xml: &str) -> Result<Layout> {
    let mut reader = Reader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);

    let mut layout: Option<Layout> = None;
    let mut current_tabpage: Option<TabPage> = None;
    let mut current_control: Option<Control> = None;

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Decl(_)) => {
                // XML declaration, skip
            }
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8(e.name().as_ref().to_vec())?;
                match tag.as_str() {
                    "layout" => {
                        let attrs = parse_raw_attrs(e.attributes())?;
                        let orientation_xml = attr_val(&attrs, "orientation")
                            .context("layout missing orientation")?;
                        let orientation = match orientation_xml.as_str() {
                            "vertical" => Orientation::Vertical,
                            "horizontal" => Orientation::Horizontal,
                            other => bail!("unknown orientation: {other}"),
                        };
                        layout = Some(Layout {
                            version: attr_val(&attrs, "version")
                                .context("layout missing version")?,
                            mode: attr_val(&attrs, "mode").context("layout missing mode")?,
                            orientation,
                            tabpages: Vec::new(),
                        });
                    }
                    "tabpage" => {
                        let attrs = parse_raw_attrs(e.attributes())?;
                        let name =
                            decode_b64(&attr_val(&attrs, "name").context("tabpage missing name")?)?;
                        let osc_cs = attr_val(&attrs, "osc_cs")
                            .map(|v| decode_b64(&v))
                            .transpose()?;
                        let li = parse_label_style(&attrs, "li")?;
                        let la = parse_label_style(&attrs, "la")?;
                        current_tabpage = Some(TabPage {
                            name,
                            scalef: attr_val(&attrs, "scalef").unwrap_or_default(),
                            scalet: attr_val(&attrs, "scalet").unwrap_or_default(),
                            osc_cs,
                            li,
                            la,
                            controls: Vec::new(),
                        });
                    }
                    "control" => {
                        let attrs = parse_raw_attrs(e.attributes())?;
                        current_control = Some(parse_control(&attrs)?);
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8(e.name().as_ref().to_vec())?;
                match tag.as_str() {
                    "midi" => {
                        // Store raw attribute bytes to preserve quirky spacing
                        // (TouchOSC uses `var ="x"` with space before `=`)
                        let raw = std::str::from_utf8(e.as_ref())?;
                        // raw looks like: `midi var ="x" type="1" ...`
                        // Strip the tag name prefix to get just the attrs
                        let raw_attrs = raw.strip_prefix("midi ").unwrap_or(raw).to_string();
                        if let Some(ref mut ctrl) = current_control {
                            ctrl.midi_bindings.push(MidiBinding { raw_attrs });
                        }
                    }
                    "control" => {
                        // Self-closing control (unlikely but handle it)
                        let attrs = parse_raw_attrs(e.attributes())?;
                        let ctrl = parse_control(&attrs)?;
                        if let Some(ref mut tp) = current_tabpage {
                            tp.controls.push(ctrl);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8(e.name().as_ref().to_vec())?;
                match tag.as_str() {
                    "control" => {
                        if let Some(ctrl) = current_control.take() {
                            if let Some(ref mut tp) = current_tabpage {
                                tp.controls.push(ctrl);
                            }
                        }
                    }
                    "tabpage" => {
                        if let Some(tp) = current_tabpage.take() {
                            if let Some(ref mut l) = layout {
                                l.tabpages.push(tp);
                            }
                        }
                    }
                    "layout" => {}
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => bail!("XML parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }

    layout.context("no <layout> element found")
}

fn parse_raw_attrs(
    attrs: quick_xml::events::attributes::Attributes,
) -> Result<Vec<(String, String)>> {
    let mut result = Vec::new();
    for attr in attrs {
        let attr = attr?;
        let key = std::str::from_utf8(attr.key.as_ref())?.to_string();
        let value = std::str::from_utf8(&attr.value)?.to_string();
        result.push((key, value));
    }
    Ok(result)
}

fn attr_val(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn decode_b64(s: &str) -> Result<String> {
    if s.is_empty() {
        return Ok(String::new());
    }
    let bytes = BASE64
        .decode(s)
        .with_context(|| format!("failed to decode base64: {s}"))?;
    String::from_utf8(bytes).with_context(|| format!("base64 decoded to invalid UTF-8: {s}"))
}

fn parse_label_style(attrs: &[(String, String)], prefix: &str) -> Result<Option<LabelStyle>> {
    let t_key = format!("{prefix}_t");
    let c_key = format!("{prefix}_c");
    let s_key = format!("{prefix}_s");
    let o_key = format!("{prefix}_o");
    let b_key = format!("{prefix}_b");

    let Some(t) = attr_val(attrs, &t_key) else {
        return Ok(None);
    };
    Ok(Some(LabelStyle {
        t: decode_b64(&t)?,
        c: attr_val(attrs, &c_key).unwrap_or_default(),
        s: attr_val(attrs, &s_key).unwrap_or_default(),
        o: attr_val(attrs, &o_key).unwrap_or_default(),
        b: attr_val(attrs, &b_key).unwrap_or_default(),
    }))
}

/// Parse a control element from its raw attributes.
///
/// The attribute order in TouchOSC XML is:
///   name, x, y, w, h, color, [scalef, scalet, osc_cs], type, [type-specific...]
///
/// We split into:
///   - Common prefix: name, x, y, w, h, color (always present)
///   - Mid attrs: anything between color and type (scalef, scalet, osc_cs)
///   - type: the control type
///   - Extra attrs: everything after type
///
/// Coordinates are stored as raw XML portrait values — see CLAUDE.md
/// for how these map to the landscape editor view.
fn parse_control(attrs: &[(String, String)]) -> Result<Control> {
    let name = decode_b64(&attr_val(attrs, "name").context("control missing name")?)?;
    let x: i32 = attr_val(attrs, "x").context("control missing x")?.parse()?;
    let y: i32 = attr_val(attrs, "y").context("control missing y")?.parse()?;
    let w: i32 = attr_val(attrs, "w").context("control missing w")?.parse()?;
    let h: i32 = attr_val(attrs, "h").context("control missing h")?.parse()?;
    let color = attr_val(attrs, "color").context("control missing color")?;
    let control_type = attr_val(attrs, "type").context("control missing type")?;

    // Collect mid_attrs (between color and type) and extra_attrs (after type)
    let common_keys = ["name", "x", "y", "w", "h", "color", "type"];
    let mut mid_attrs = Vec::new();
    let mut extra_attrs = Vec::new();
    let mut past_type = false;
    for (k, v) in attrs {
        if common_keys.contains(&k.as_str()) {
            if k == "type" {
                past_type = true;
            }
            continue;
        }
        if past_type {
            extra_attrs.push((k.clone(), v.clone()));
        } else {
            mid_attrs.push((k.clone(), v.clone()));
        }
    }

    // Decode base64 values in mid_attrs (osc_cs)
    let mid_attrs = mid_attrs
        .into_iter()
        .map(|(k, v)| {
            if k == "osc_cs" {
                Ok((k, decode_b64(&v)?))
            } else {
                Ok((k, v))
            }
        })
        .collect::<Result<Vec<_>>>()?;

    // Decode base64 values in extra_attrs (text)
    let extra_attrs = extra_attrs
        .into_iter()
        .map(|(k, v)| {
            if k == "text" {
                Ok((k, decode_b64(&v)?))
            } else {
                Ok((k, v))
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Control {
        name,
        x,
        y,
        w,
        h,
        color,
        control_type,
        mid_attrs,
        extra_attrs,
        midi_bindings: Vec::new(),
    })
}
