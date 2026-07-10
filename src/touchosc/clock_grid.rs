//! Resize and re-label the animation page's clock-source selector to match the
//! number of clocks in use.

use anyhow::{Context, Result};

use super::model::{Control, Layout};

const ANIMATION_PAGE: &str = "animation";
const CLOCK_SOURCE_OSC: &str = "/Animation/ClockSource";

/// Update the animation page's clock-source selector for `n_clocks` clocks.
///
/// Sets the button grid to `n_clocks + 1` buttons (the extra one is the
/// "internal" option) and regenerates the labels overlaid on it ("internal",
/// "clock 1" … "clock N"), one centred on each button. The grid's position and
/// size and the labels' style are read from the layout, so moving or restyling
/// the selector in the base template carries through automatically.
pub fn set_clock_source_grid(layout: &mut Layout, n_clocks: usize) -> Result<()> {
    let n_buttons = n_clocks + 1;

    let page = layout
        .tabpages
        .iter_mut()
        .find(|tp| tp.name == ANIMATION_PAGE)
        .with_context(|| format!("layout has no '{ANIMATION_PAGE}' page"))?;

    // Read the grid's y-extent from the multipush — never assume a fixed frame.
    let (grid_y, grid_h) = {
        let grid = page
            .controls
            .iter()
            .find(|c| is_clock_source_grid(c))
            .with_context(|| format!("'{ANIMATION_PAGE}' page has no {CLOCK_SOURCE_OSC} grid"))?;
        (grid.y, grid.h)
    };

    // Clone an existing clock label to inherit its style (x, w, colour, size,
    // background, outline). Its `h` caps the generated label height.
    let example = page
        .controls
        .iter()
        .find(|c| is_clock_label(c))
        .context("clock-source grid has no example label to derive style from")?
        .clone();

    // Resize the grid.
    if let Some(grid) = page.controls.iter_mut().find(|c| is_clock_source_grid(c)) {
        set_extra(grid, "number_y", &n_buttons.to_string());
    }

    // Replace the labels with one centred on each button.
    page.controls.retain(|c| !is_clock_label(c));
    for i in 0..n_buttons {
        page.controls
            .push(make_label(&example, grid_y, grid_h, n_buttons, i));
    }

    Ok(())
}

fn is_clock_source_grid(c: &Control) -> bool {
    c.control_type == "multipush" && c.osc_address() == Some(CLOCK_SOURCE_OSC)
}

fn is_clock_label(c: &Control) -> bool {
    c.control_type == "labelv"
        && get_extra(c, "text").is_some_and(|t| t == "internal" || is_clock_n(t))
}

/// True for `"clock <n>"` where `<n>` is a non-empty run of digits.
fn is_clock_n(text: &str) -> bool {
    text.strip_prefix("clock ")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Build a label centred on button `i` of an `n_buttons`-button grid spanning
/// `[grid_y, grid_y + grid_h)` along the y axis, cloning `example`'s style.
fn make_label(example: &Control, grid_y: i32, grid_h: i32, n_buttons: usize, i: usize) -> Control {
    let band = grid_h as f64 / n_buttons as f64;
    let center = grid_y as f64 + (i as f64 + 0.5) * band;
    // Cap at the template label's height; shrink to the band so labels never
    // overlap when there are many buttons.
    let h = example.h.min(band as i32);
    let y = (center - h as f64 / 2.0).round() as i32;
    let text = if i == 0 {
        "internal".to_string()
    } else {
        format!("clock {i}")
    };

    let mut label = example.clone();
    label.name = format!("clocklabel{i}");
    label.y = y;
    label.h = h;
    set_extra(&mut label, "text", &text);
    label
}

fn get_extra<'a>(c: &'a Control, key: &str) -> Option<&'a str> {
    c.extra_attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn set_extra(c: &mut Control, key: &str, value: &str) {
    if let Some(entry) = c.extra_attrs.iter_mut().find(|(k, _)| k == key) {
        entry.1 = value.to_string();
    } else {
        c.extra_attrs.push((key.to_string(), value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::templates::BASE_TEMPLATE;
    use super::*;

    fn base() -> Layout {
        BASE_TEMPLATE.parse().expect("base template parses")
    }

    fn animation_page(layout: &Layout) -> &super::super::model::TabPage {
        layout
            .tabpages
            .iter()
            .find(|tp| tp.name == ANIMATION_PAGE)
            .unwrap()
    }

    fn clock_labels(layout: &Layout) -> Vec<(&str, i32, i32)> {
        animation_page(layout)
            .controls
            .iter()
            .filter(|c| is_clock_label(c))
            .map(|c| (get_extra(c, "text").unwrap(), c.y, c.h))
            .collect()
    }

    fn grid_number_y(layout: &Layout) -> usize {
        let grid = animation_page(layout)
            .controls
            .iter()
            .find(|c| is_clock_source_grid(c))
            .unwrap();
        get_extra(grid, "number_y").unwrap().parse().unwrap()
    }

    #[test]
    fn resizes_grid_and_labels_for_each_count() {
        for n in [4usize, 8, 12] {
            let mut layout = base();
            set_clock_source_grid(&mut layout, n).unwrap();

            assert_eq!(
                grid_number_y(&layout),
                n + 1,
                "{n} clocks -> {} buttons",
                n + 1
            );

            let labels = clock_labels(&layout);
            assert_eq!(labels.len(), n + 1);
            assert_eq!(labels[0].0, "internal");
            assert_eq!(labels[1].0, "clock 1");
            assert_eq!(labels[n].0, format!("clock {n}"));

            // Labels are ordered top-to-bottom (increasing y) and never overlap.
            for pair in labels.windows(2) {
                assert!(
                    pair[0].1 + pair[0].2 <= pair[1].1 + 1,
                    "labels overlap at {n}: {pair:?}"
                );
            }
        }
    }

    #[test]
    fn label_positions_track_the_grid_frame() {
        // Move the grid; the regenerated labels must follow it, proving no
        // position is hardcoded.
        let mut moved = base();
        let shift = 100;
        {
            let page = moved
                .tabpages
                .iter_mut()
                .find(|tp| tp.name == ANIMATION_PAGE)
                .unwrap();
            let grid = page
                .controls
                .iter_mut()
                .find(|c| is_clock_source_grid(c))
                .unwrap();
            grid.y += shift;
        }

        let mut base_layout = base();
        set_clock_source_grid(&mut base_layout, 8).unwrap();
        set_clock_source_grid(&mut moved, 8).unwrap();

        let base_ys: Vec<i32> = clock_labels(&base_layout).iter().map(|l| l.1).collect();
        let moved_ys: Vec<i32> = clock_labels(&moved).iter().map(|l| l.1).collect();
        assert_eq!(base_ys.len(), moved_ys.len());
        for (b, m) in base_ys.iter().zip(&moved_ys) {
            assert_eq!(*m, *b + shift, "labels did not follow the moved grid");
        }
    }

    #[test]
    fn round_trips_to_valid_touchosc() {
        let mut layout = base();
        set_clock_source_grid(&mut layout, 12).unwrap();
        // Serialize -> zip -> parse; the result must be structurally intact.
        let reparsed = layout.to_zip().unwrap().parse().unwrap();
        assert_eq!(grid_number_y(&reparsed), 13);
        assert_eq!(clock_labels(&reparsed).len(), 13);
    }
}
