#!/usr/bin/env python3
"""Generate app icons and splash screen from the source PDF.

Requirements: pip install Pillow; brew install poppler

The source PDF (emblem.pdf) should be tightly cropped to the artwork.

Strategy:
- Rasterize once at very high resolution (8192px wide) via pdftoppm.
- Trim all whitespace to find the exact content bounding box.
- For icons: invert, pad top/bottom to make a square, then downsample to each size.
- For splash: rasterize at a DPI giving an even pixel width near 600px, invert,
  apply luminance-as-alpha for transparency.

Usage:
    python3 resources/generate_icons.py

Inputs:
    resources/emblem.pdf       — source vector art (black on white)

Outputs:
    resources/splash.png       — white insignia on transparent background
    resources/icons/icon_N.png — square icons at 16, 32, 64, 128, 256, 512, 1024px
        - 128px and above: full mark, white on black, centered in square
        - 64px and below:  cropped to center three cobra heads, white on black
"""

import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageChops, ImageOps

SCRIPT_DIR = Path(__file__).parent
PDF_PATH = SCRIPT_DIR / "emblem.pdf"
ICON_DIR = SCRIPT_DIR / "icons"

# Target splash width (approximate — adjusted to nearest even pixel width).
SPLASH_WIDTH_APPROX = 600

# High-res rasterization width for icons.
HIRES_WIDTH = 8192

# Icon sizes.
LARGE_ICON_SIZES = [1024, 512, 256, 128]
SMALL_ICON_SIZES = [64, 32, 16]

# Crop region for small icons (center three cobra heads + upper stripes).
# Expressed as fractions of the trimmed artwork dimensions (before square padding).
CROP_LEFT = 0.30
CROP_RIGHT = 0.70
CROP_TOP = 0.03
CROP_BOTTOM = 0.55


# ---------------------------------------------------------------------------
# PDF rasterization
# ---------------------------------------------------------------------------


def get_pdf_page_width_points(pdf_path: Path) -> float:
    """Get the page width in points from a PDF using pdfinfo."""
    result = subprocess.run(
        ["pdfinfo", str(pdf_path)],
        check=True, capture_output=True, text=True,
    )
    for line in result.stdout.splitlines():
        if line.startswith("Page size:"):
            return float(line.split()[2])
    raise ValueError(f"Could not determine page size from {pdf_path}")


def rasterize_pdf(pdf_path: Path, dpi: int) -> Image.Image:
    """Rasterize a single-page PDF at the given DPI using pdftoppm."""
    with tempfile.TemporaryDirectory() as tmp:
        prefix = str(Path(tmp) / "page")
        subprocess.run(
            ["pdftoppm", "-png", "-r", str(dpi), "-singlefile",
             str(pdf_path), prefix],
            check=True, capture_output=True,
        )
        return Image.open(f"{prefix}.png").copy()


def dpi_for_width(pdf_path: Path, target_width: int) -> int:
    """Compute the DPI needed to rasterize at approximately target_width pixels."""
    page_width_pts = get_pdf_page_width_points(pdf_path)
    return round(target_width / (page_width_pts / 72.0))


def dpi_for_even_width(pdf_path: Path, target_width: int) -> int:
    """Find a DPI that produces an even pixel width close to target_width."""
    page_width_pts = get_pdf_page_width_points(pdf_path)
    base_dpi = round(target_width / (page_width_pts / 72.0))
    for offset in range(0, 10):
        for dpi in [base_dpi + offset, base_dpi - offset]:
            if dpi < 1:
                continue
            pixel_width = round(dpi * page_width_pts / 72.0)
            if pixel_width % 2 == 0:
                return dpi
    return base_dpi


# ---------------------------------------------------------------------------
# Image operations
# ---------------------------------------------------------------------------


def trim_whitespace(img: Image.Image) -> Image.Image:
    """Trim all white padding from all sides."""
    bg = Image.new(img.mode, img.size, (255, 255, 255))
    diff = ImageChops.difference(img, bg)
    bbox = diff.getbbox()
    if bbox is None:
        return img
    return img.crop(bbox)


def to_splash(img: Image.Image) -> Image.Image:
    """Convert inverted RGB image to white + luminance-as-alpha.

    Uses luminance as alpha: bright pixels become opaque white, dark pixels
    become transparent, and anti-aliased gray edges get proportional alpha.
    """
    gray = img.convert("L")
    white = Image.new("RGB", img.size, (255, 255, 255))
    rgba = white.convert("RGBA")
    rgba.putalpha(gray)
    return rgba


def pad_to_square(img: Image.Image, bg_color: tuple[int, ...]) -> Image.Image:
    """Center image in a square with the given background color."""
    w, h = img.size
    size = max(w, h)
    square = Image.new(img.mode, (size, size), bg_color)
    square.paste(img, ((size - w) // 2, (size - h) // 2))
    return square


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main():
    ICON_DIR.mkdir(exist_ok=True)

    # --- Splash screen ---
    splash_dpi = dpi_for_even_width(PDF_PATH, SPLASH_WIDTH_APPROX)
    print(f"Generating splash screen (DPI={splash_dpi})...")
    splash_raw = rasterize_pdf(PDF_PATH, splash_dpi)
    splash_inv = ImageOps.invert(splash_raw)
    splash = to_splash(splash_inv)
    splash_path = str(SCRIPT_DIR / "splash.png")
    splash.save(splash_path)
    print(f"  -> {splash_path} ({splash.size[0]}x{splash.size[1]})")

    # --- Icons ---
    hires_dpi = dpi_for_width(PDF_PATH, HIRES_WIDTH)
    print(f"Rasterizing high-res source (DPI={hires_dpi})...")
    hires = rasterize_pdf(PDF_PATH, hires_dpi)
    print(f"  Rasterized: {hires.size[0]}x{hires.size[1]}")

    # Trim all whitespace to content bounding box.
    trimmed = trim_whitespace(hires)
    tw, th = trimmed.size
    print(f"  Trimmed: {tw}x{th}")

    # Invert: white artwork on black.
    inverted = ImageOps.invert(trimmed)

    # Pad to square (artwork is wider than tall, so adds black top/bottom).
    square = pad_to_square(inverted, (0, 0, 0))
    print(f"  Square: {square.size[0]}x{square.size[1]}")

    # Large icons: downsample from the high-res square.
    for px in LARGE_ICON_SIZES:
        out = str(ICON_DIR / f"icon_{px}.png")
        square.resize((px, px), Image.LANCZOS).save(out)
        print(f"  -> {out}")

    # Small icons: square crop centered on the three cobra heads.
    left = int(tw * CROP_LEFT)
    right = int(tw * CROP_RIGHT)
    top = int(th * CROP_TOP)
    crop_w = right - left
    # Force square: use crop width as the height too.
    bottom = top + crop_w
    cropped = inverted.crop((left, top, right, bottom))
    for px in SMALL_ICON_SIZES:
        out = str(ICON_DIR / f"icon_{px}.png")
        cropped.resize((px, px), Image.LANCZOS).save(out)
        print(f"  -> {out}")

    print("Done.")


if __name__ == "__main__":
    main()
