#!/usr/bin/env python3
"""Draw the Windows installer's images into windows/packaging/art/ (run after changing the icon),
as TurboDM's: Mint's green, the app's icon, its name. Needs Pillow and the Segoe UI font
(Windows' own, or SEGOE_DIR)."""
import os
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "packaging/art"
WINDOWS_FONTS = Path(os.environ.get("WINDIR", "C:/Windows")) / "Fonts"
FONTS = Path(os.environ.get("SEGOE_DIR", WINDOWS_FONTS))
TOP, BOTTOM = (52, 168, 84), (22, 104, 52)  # Mint's green, darker below (the icon stands out)


def icon(size):
    """The app's icon at `size`, from the largest picture in its .ico."""
    ico = Image.open(ROOT / "data/hotspot-manager.ico")
    biggest = max(ico.info.get("sizes", [ico.size]))
    ico.size = biggest
    return ico.convert("RGBA").resize((size, size), Image.LANCZOS)


def gradient(w, h):
    image = Image.new("RGB", (w, h))
    px = image.load()
    for y in range(h):
        for x in range(w):
            t = min(1.0, (0.75 * y / h + 0.25 * x / w))
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM))
    return image


def banner(scale):
    """Left side of the welcome and finish pages (164x314 at 100%)."""
    w, h = round(164 * scale), round(314 * scale)
    image = gradient(w, h)
    overlay = Image.new("RGBA", image.size, (0, 0, 0, 0))
    mark = icon(round(88 * scale))
    left, top = (w - mark.width) / 2, 78 * scale
    # a soft shadow lifts the icon off the gradient
    shadow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [left + 2 * scale, top + 6 * scale, left + mark.width - 2 * scale, top + mark.height + 4 * scale],
        radius=22 * scale, fill=(5, 40, 15, 110))
    overlay = Image.alpha_composite(overlay, shadow.filter(ImageFilter.GaussianBlur(8 * scale)))
    overlay.alpha_composite(mark, (round(left), round(top)))
    image = Image.alpha_composite(image.convert("RGBA"), overlay)
    draw = ImageDraw.Draw(image)
    title = ImageFont.truetype(str(FONTS / "segoeuib.ttf"), round(17.5 * scale))
    sub = ImageFont.truetype(str(FONTS / "segoeui.ttf"), round(11.5 * scale))
    draw.text((w / 2, top + mark.height + 26 * scale), "Hotspot Manager", font=title, fill="white", anchor="mt")
    draw.text((w / 2, top + mark.height + 54 * scale), "Wi-Fi hotspot", font=sub,
              fill=(255, 255, 255, 215), anchor="mt")
    return image.convert("RGB")


def small(scale):
    """Top right of the other pages (55x55 at 100%), with transparency."""
    size = round(55 * scale)
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    mark = icon(round(48 * scale))
    image.alpha_composite(mark, ((size - mark.width) // 2, (size - mark.height) // 2))
    return image


OUT.mkdir(parents=True, exist_ok=True)
for percent in (100, 125, 150, 175, 200, 250):
    scale = percent / 100
    banner(scale).save(OUT / f"wizard-{percent}.bmp")
    small(scale).save(OUT / f"wizard-small-{percent}.bmp")
print("installer art written to", OUT)
