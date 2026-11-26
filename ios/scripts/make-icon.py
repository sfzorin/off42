#!/usr/bin/env python3
"""Draws the 1024 × 1024 App Store / home screen icon from the site's "42" mark (website/public/assets/favicon.svg):
the same strokes and colours on a full square, since iOS applies its own corner mask. Needs Pillow.

    python3 ios/scripts/make-icon.py      (writes ios/OFF42/Assets.xcassets/AppIcon.appiconset/icon-1024.png)
"""
import pathlib

from PIL import Image, ImageDraw

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "ios" / "OFF42" / "Assets.xcassets" / "AppIcon.appiconset" / "icon-1024.png"

SIZE = 1024
SUPER = 4  # drawn at 4× and scaled down for smooth edges
UNIT = SIZE * SUPER / 64  # the SVG's viewBox is 64 × 64
WIDTH = 6  # stroke width in viewBox units

BACKGROUND = (0x17, 0x3B, 0x30)
FOUR = (0xFF, 0xF9, 0xEF)
TWO = (0xFF, 0xAB, 0x80)


def cubic(p0, p1, p2, p3, steps=48):
    for i in range(1, steps + 1):
        t = i / steps
        a, b, c, d = (1 - t) ** 3, 3 * (1 - t) ** 2 * t, 3 * (1 - t) * t ** 2, t ** 3
        yield (a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1])


def stroke(draw, points, colour):
    """A polyline with round caps and joins, like the SVG's stroke-linecap/linejoin="round"."""
    scaled = [(x * UNIT, y * UNIT) for x, y in points]
    width = WIDTH * UNIT
    draw.line(scaled, fill=colour, width=round(width))
    radius = width / 2
    for x, y in scaled:
        draw.ellipse((x - radius, y - radius, x + radius, y + radius), fill=colour)


def main():
    image = Image.new("RGB", (SIZE * SUPER, SIZE * SUPER), BACKGROUND)
    draw = ImageDraw.Draw(image)
    # "4": M23 16 10 35h19 and m-6-19v32
    stroke(draw, [(23, 16), (10, 35), (29, 35)], FOUR)
    stroke(draw, [(23, 16), (23, 48)], FOUR)
    # "2": M36 23 c0-4 4-7 9-7 s9 3 9 8 c0 9-17 13-18 24 h18
    two = [(36, 23)]
    two += cubic((36, 23), (36, 19), (40, 16), (45, 16))
    # the "s" command reflects the previous control point (40,16) about (45,16): (50,16)
    two += cubic((45, 16), (50, 16), (54, 19), (54, 24))
    two += cubic((54, 24), (54, 33), (37, 37), (36, 48))
    two += [(54, 48)]
    stroke(draw, two, TWO)
    image = image.resize((SIZE, SIZE), Image.LANCZOS)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    image.save(OUT, optimize=True)
    print(f"Wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
