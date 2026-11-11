#!/usr/bin/env python3
"""Render the Moonframe build's icon from crates/sheprd-gpui/moonframe/assets/icon.svg.

Writes icon.png (1024px, the About image and Moonframe's Dock icon) and
Sheprd-Moonframe.icns beside it. Each size is rendered from the vector with
the same macOS margins as Sheprd's own icon (scripts/generate-icons.swift),
whose tile is also 896px on a 1024px canvas. Needs rsvg-convert (librsvg)
and iconutil (macOS).
"""

import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates" / "sheprd-gpui" / "moonframe" / "assets"
SOURCE = ASSETS / "icon.svg"
# Pixel margins of Apple's app icon template, by size, as Sheprd's icon uses.
INSETS = {16: 1, 32: 2, 64: 6, 128: 12, 256: 25, 512: 50, 1024: 100}


def render(pixels: int, output: Path) -> None:
    width = (pixels - 2 * INSETS[pixels]) * 1024 / 896
    offset = (pixels - width) / 2
    subprocess.run(
        [
            "rsvg-convert",
            "--width", str(width), "--height", str(width),
            "--page-width", str(pixels), "--page-height", str(pixels),
            "--left", str(offset), "--top", str(offset),
            "--output", str(output), str(SOURCE),
        ],
        check=True,
    )


def main() -> None:
    render(1024, ASSETS / "icon.png")
    with tempfile.TemporaryDirectory() as temporary:
        iconset = Path(temporary) / "Sheprd-Moonframe.iconset"
        iconset.mkdir()
        for points in (16, 32, 128, 256, 512):
            render(points, iconset / f"icon_{points}x{points}.png")
            render(points * 2, iconset / f"icon_{points}x{points}@2x.png")
        output = Path(temporary) / "Sheprd-Moonframe.icns"
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(output)], check=True)
        shutil.copy(output, ASSETS / "Sheprd-Moonframe.icns")


if __name__ == "__main__":
    main()
