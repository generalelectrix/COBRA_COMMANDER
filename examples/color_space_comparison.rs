//! Throwaway PNG generator that compares how HSV, HSI, and HSLuv each render
//! into RGB and RGBW emitter spaces.
//!
//! Output layout (rows × cols):
//!     HSV   RGB | HSV   RGBW
//!     HSI   RGB | HSI   RGBW
//!     HSLuv RGB | HSLuv RGBW
//!
//! Each panel is a hue × saturation grid. Each cell is a perceived-color
//! swatch on top with grayscale per-diode strips beneath, so the W diode's
//! contribution is visible even on an sRGB monitor.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use cobra_commander::color::{
    HSLUV_LIGHTNESS_OFFSET, Hsi, Hsluv, Hsv, RenderColor, W_DIODE_BRIGHTNESS,
};
use image::{ImageBuffer, Rgb};
use number::{Phase, UnipolarFloat};

#[derive(Parser)]
struct Args {
    /// Output PNG path.
    #[clap(long, default_value = "target/color_space_comparison.png")]
    output: PathBuf,

    /// HSLuv lightness. Defaults to [`HSLUV_LIGHTNESS_OFFSET`] — the level at
    /// which all three primaries are at the brightness of full blue, which
    /// produces the most chromatically informative comparison.
    #[clap(long)]
    hsluv_lightness: Option<f64>,

    /// W-diode brightness ratio used when simulating RGBW output on an sRGB
    /// monitor. Defaults to [`W_DIODE_BRIGHTNESS`] — set this to investigate
    /// what the conversion looks like on a fixture with a different real ratio.
    #[clap(long)]
    simulated_kw: Option<f64>,

    /// Hue samples per panel (cell columns).
    #[clap(long, default_value_t = 60)]
    hue_steps: u32,

    /// Saturation samples per panel (cell rows; sat = 1 at the top).
    #[clap(long, default_value_t = 8)]
    sat_steps: u32,

    /// Cell width in pixels.
    #[clap(long, default_value_t = 16)]
    cell_w: u32,

    /// Swatch (perceived color) height in pixels at the top of each cell.
    #[clap(long, default_value_t = 24)]
    swatch_h: u32,

    /// Per-diode strip height in pixels (3 strips for RGB, 4 for RGBW).
    #[clap(long, default_value_t = 6)]
    strip_h: u32,

    /// Gutter in pixels between panels.
    #[clap(long, default_value_t = 16)]
    gutter: u32,
}

#[derive(Clone, Copy)]
enum Space {
    Hsv,
    Hsi,
    Hsluv,
}

impl Space {
    const ALL: [Space; 3] = [Space::Hsv, Space::Hsi, Space::Hsluv];
    fn label(self) -> &'static str {
        match self {
            Space::Hsv => "HSV",
            Space::Hsi => "HSI",
            Space::Hsluv => "HSLuv",
        }
    }
}

#[derive(Clone, Copy)]
enum Gamut {
    Rgb,
    Rgbw,
}

impl Gamut {
    const ALL: [Gamut; 2] = [Gamut::Rgb, Gamut::Rgbw];
    fn n_diodes(self) -> u32 {
        match self {
            Gamut::Rgb => 3,
            Gamut::Rgbw => 4,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Gamut::Rgb => "RGB",
            Gamut::Rgbw => "RGBW",
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let lightness = match args.hsluv_lightness {
        Some(x) => UnipolarFloat::new(x.clamp(0.0, 1.0)),
        None => HSLUV_LIGHTNESS_OFFSET,
    };
    let simulated_kw = args.simulated_kw.unwrap_or(W_DIODE_BRIGHTNESS);

    // Cells differ in height between RGB (3 strips) and RGBW (4 strips); pick
    // the larger so panel rows align even though the row content differs.
    let cell_h_max = args.swatch_h + 4 * args.strip_h;
    let panel_w = args.cell_w * args.hue_steps;
    let panel_h = cell_h_max * args.sat_steps;

    let n_cols = Gamut::ALL.len() as u32;
    let n_rows = Space::ALL.len() as u32;
    let total_w = n_cols * panel_w + (n_cols + 1) * args.gutter;
    let total_h = n_rows * panel_h + (n_rows + 1) * args.gutter;

    let bg = Rgb([24u8, 24, 24]);
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(total_w, total_h, bg);

    for (row, space) in Space::ALL.iter().enumerate() {
        for (col, gamut) in Gamut::ALL.iter().enumerate() {
            let panel_x = args.gutter + (col as u32) * (panel_w + args.gutter);
            let panel_y = args.gutter + (row as u32) * (panel_h + args.gutter);
            render_panel(
                &mut img,
                panel_x,
                panel_y,
                *space,
                *gamut,
                &args,
                lightness,
                simulated_kw,
            );
        }
    }

    if let Some(parent) = args.output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    img.save(&args.output)?;

    println!(
        "wrote {} ({}x{} px)",
        args.output.display(),
        total_w,
        total_h
    );
    println!(
        "  rows (top→bottom): {}",
        Space::ALL
            .iter()
            .map(|s| s.label())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  cols (left→right): {}",
        Gamut::ALL
            .iter()
            .map(|g| g.label())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  per cell: swatch on top, then per-diode grayscale strips (R, G, B[, W])");
    println!("  hsluv lightness: {:.4}", lightness.val());
    println!("  simulated kw:    {:.4}", simulated_kw);
    Ok(())
}

fn render_panel(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    panel_x: u32,
    panel_y: u32,
    space: Space,
    gamut: Gamut,
    args: &Args,
    lightness: UnipolarFloat,
    simulated_kw: f64,
) {
    let n_strips = gamut.n_diodes();
    let cell_h = args.swatch_h + 4 * args.strip_h; // align row heights across gamuts

    for sy in 0..args.sat_steps {
        let sat = sat_for_row(sy, args.sat_steps);
        for hx in 0..args.hue_steps {
            let hue = Phase::new((hx as f64) / (args.hue_steps as f64));
            let cell = build_cell(space, gamut, hue, sat, lightness, simulated_kw);

            let cx = panel_x + hx * args.cell_w;
            let cy = panel_y + sy * cell_h;

            fill_rect(img, cx, cy, args.cell_w, args.swatch_h, Rgb(cell.swatch));
            for (i, drive) in cell.diodes.iter().enumerate().take(n_strips as usize) {
                let g = *drive;
                let strip_y = cy + args.swatch_h + (i as u32) * args.strip_h;
                fill_rect(img, cx, strip_y, args.cell_w, args.strip_h, Rgb([g, g, g]));
            }
        }
    }
}

fn sat_for_row(sy: u32, sat_steps: u32) -> UnipolarFloat {
    if sat_steps <= 1 {
        UnipolarFloat::ONE
    } else {
        let t = (sy as f64) / ((sat_steps - 1) as f64);
        UnipolarFloat::new((1.0 - t).clamp(0.0, 1.0))
    }
}

struct Cell {
    swatch: [u8; 3],
    diodes: [u8; 4], // index 3 is W; ignored for RGB gamut
}

fn build_cell(
    space: Space,
    gamut: Gamut,
    hue: Phase,
    sat: UnipolarFloat,
    lightness: UnipolarFloat,
    simulated_kw: f64,
) -> Cell {
    match (space, gamut) {
        (Space::Hsv, Gamut::Rgb) => {
            let c = Hsv {
                hue,
                sat,
                val: UnipolarFloat::ONE,
            };
            cell_from_rgb(c.rgb())
        }
        (Space::Hsv, Gamut::Rgbw) => {
            let c = Hsv {
                hue,
                sat,
                val: UnipolarFloat::ONE,
            };
            cell_from_rgbw(c.rgbw(), simulated_kw)
        }
        (Space::Hsi, Gamut::Rgb) => {
            let c = Hsi {
                hue,
                sat,
                intensity: UnipolarFloat::ONE,
            };
            cell_from_rgb(c.rgb())
        }
        (Space::Hsi, Gamut::Rgbw) => {
            let c = Hsi {
                hue,
                sat,
                intensity: UnipolarFloat::ONE,
            };
            cell_from_rgbw(c.rgbw(), simulated_kw)
        }
        (Space::Hsluv, Gamut::Rgb) => {
            let c = Hsluv {
                hue,
                sat,
                lightness,
            };
            cell_from_rgb(c.rgb())
        }
        (Space::Hsluv, Gamut::Rgbw) => {
            let c = Hsluv {
                hue,
                sat,
                lightness,
            };
            cell_from_rgbw(c.rgbw(), simulated_kw)
        }
    }
}

fn cell_from_rgb(rgb: [u8; 3]) -> Cell {
    Cell {
        swatch: rgb,
        diodes: [rgb[0], rgb[1], rgb[2], 0],
    }
}

fn cell_from_rgbw(rgbw: [u8; 4], kw: f64) -> Cell {
    let r = rgbw[0] as f64 / 255.0;
    let g = rgbw[1] as f64 / 255.0;
    let b = rgbw[2] as f64 / 255.0;
    let w = rgbw[3] as f64 / 255.0;
    let contrib = w * kw / 3.0;
    let to_u8 = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Cell {
        swatch: [to_u8(r + contrib), to_u8(g + contrib), to_u8(b + contrib)],
        diodes: rgbw,
    }
}

fn fill_rect(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: Rgb<u8>,
) {
    let (img_w, img_h) = (img.width(), img.height());
    for dy in 0..h {
        let py = y + dy;
        if py >= img_h {
            break;
        }
        for dx in 0..w {
            let px = x + dx;
            if px >= img_w {
                break;
            }
            img.put_pixel(px, py, color);
        }
    }
}
