#!/usr/bin/env python3
"""The Windows app's pictures: the app's icon (from icons/hotspot-manager.svg) as an .ico, and
3D tiles for the device list (a rounded tile in a colour gradient with a white Fluent UI symbol,
MIT: data/icons/symbols/LICENSE), the same style as TurboDM's. Writes:

  data/hotspot-manager.ico               the app's icon
  data/icons/<name>.png (64 px)          device pictures

Needs resvg (`cargo install resvg`) on the PATH. Run from anywhere: python3 windows/tools/icons.py
"""
import re
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "data/icons"

# name: ((light, dark), symbol)
TILES = {
    "phone": (("#5fd17a", "#26914a"), "phone"),
    "laptop": (("#62a8f7", "#2369d1"), "laptop"),
    "device": (("#52d3c7", "#178f88"), "phone_laptop"),
}


def symbol(name):
    """The symbol's drawing (24 × 24), in white."""
    svg = (ICONS / "symbols" / f"{name}.svg").read_text()
    body = re.search(r"<svg[^>]*>(.*)</svg>", svg, re.S).group(1)
    return re.sub(r'fill="#[0-9a-fA-F]+"', 'fill="#fff"', body).strip()


def tile(colours, glyph, size=64, inset=5, radius=13):
    light, dark = colours
    side = size - 2 * inset
    top = inset - 1
    scale = side * 0.6 / 24
    offset = (size - 24 * scale) / 2
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">
  <defs>
    <linearGradient id="fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{dark}"/>
    </linearGradient>
    <linearGradient id="shine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity="0.42"/><stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <clipPath id="shape"><rect x="{inset}" y="{top}" width="{side}" height="{side}" rx="{radius}"/></clipPath>
    <filter id="shadow" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="0" dy="{size / 40:.2f}" stdDeviation="{size / 40:.2f}" flood-color="#000" flood-opacity="0.32"/>
    </filter>
  </defs>
  <rect x="{inset}" y="{top}" width="{side}" height="{side}" rx="{radius}" fill="url(#fill)" filter="url(#shadow)"/>
  <rect x="{inset}" y="{top}" width="{side}" height="{side / 2:.1f}" fill="url(#shine)" clip-path="url(#shape)"/>
  <rect x="{inset + 0.5}" y="{top + 0.5}" width="{side - 1}" height="{side - 1}" rx="{radius - 0.5}" fill="none"
        stroke="{dark}" stroke-opacity="0.45"/>
  <g transform="translate({offset:.2f} {offset - 1:.2f}) scale({scale:.4f})">
    {glyph}
  </g>
</svg>
'''


def render(svg_path, png_path, size):
    subprocess.run(["resvg", "-w", str(size), "-h", str(size), str(svg_path), str(png_path)], check=True)


def ico(pngs, path):
    """An .ico of PNG images (Windows Vista and later read those at every size)."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries, data, offset = b"", b"", 6 + 16 * len(pngs)
    for size, png in pngs:
        byte = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", byte, byte, 0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    path.write_bytes(header + entries + data)


def main():
    tmp = ROOT / "target/icon-render"
    tmp.mkdir(parents=True, exist_ok=True)
    for name, (colours, sym) in TILES.items():
        svg = tmp / f"{name}.svg"
        svg.write_text(tile(colours, symbol(sym)))
        render(svg, ICONS / f"{name}.png", 64)
    app = ROOT.parent / "icons/hotspot-manager.svg"
    pngs = []
    for size in (16, 20, 24, 32, 40, 48, 64, 96, 128, 256):
        out = tmp / f"{size}.png"
        render(app, out, size)
        pngs.append((size, out.read_bytes()))
    ico(pngs, ROOT / "data/hotspot-manager.ico")


if __name__ == "__main__":
    main()
