#!/usr/bin/env python3
# Adapted from zeron scripts/dmg-background.py (render(), W/H geometry and the 1x/2x pair) (MIT).
"""Render the dmg background: tern's dusk sky, a soft sun glow, a curved arrow from the app slot
to the Applications slot and a one-line hint, for the 660x400 pt drag-to-Applications window.

Writes packaging/macos/dmg-background.png (1x) and dmg-background@2x.png; scripts/release.sh
pairs them into a hidpi tiff with tiffutil. Both renders are committed so a release never needs
Pillow; rerun this only when changing the artwork.

Usage: python3 scripts/dmg-background.py   (needs Pillow)
"""

import math
import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
FONT = os.path.join(ROOT, "crates/tern/assets/fonts/Geist-Medium.ttf")
OUT = os.path.join(ROOT, "packaging/macos")

# Window and icon geometry in points; must match the dmgbuild settings in scripts/release.sh.
W, H = 660, 400
APP = (165, 170)
APPLICATIONS = (495, 170)

# The icon's dusk sky (packaging/macos/icon.svg), compressed so the Finder labels under the
# icons (y ~ 230..250) fall on the light salmon band where black label text stays readable.
SKY = [
    (0.00, (0x12, 0x1A, 0x44)),
    (0.22, (0x2F, 0x3A, 0x86)),
    (0.42, (0x7B, 0x5A, 0xA6)),
    (0.60, (0xE4, 0x8A, 0x8A)),
    (1.00, (0xF7, 0xBF, 0x86)),
]


def sky(t):
    for (t0, c0), (t1, c1) in zip(SKY, SKY[1:]):
        if t <= t1:
            f = (t - t0) / (t1 - t0)
            return tuple(round(a + (b - a) * f) for a, b in zip(c0, c1))
    return SKY[-1][1]


def bezier(p0, p1, p2, n=80):
    return [
        (
            (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t**2 * p2[0],
            (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t**2 * p2[1],
        )
        for t in (i / n for i in range(n + 1))
    ]


def render(s, path):
    """Render at pixel scale s (1 or 2); drawing is supersampled 4x for smooth edges."""
    ss = s * 4
    w, h = W * ss, H * ss
    img = Image.new("RGB", (w, h))
    px = ImageDraw.Draw(img)
    for y in range(h):
        px.line([(0, y), (w, y)], fill=sky(y / (h - 1)))

    # Soft sun glow on the horizon, between the two icons.
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    cx, cy, r = W / 2 * ss, (H + 10) * ss, 190 * ss
    for i in range(40, 0, -1):
        f = i / 40
        a = round(150 * (1 - f) ** 2)
        rr = r * f
        gd.ellipse([cx - rr, cy - rr * 0.8, cx + rr, cy + rr * 0.8], fill=(255, 236, 190, a))
    glow = glow.filter(ImageFilter.GaussianBlur(14 * ss))
    img.paste(glow, (0, 0), glow)

    # Thin white arrow: a curve over the top from app slot to Applications slot.
    layer = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    start = (APP[0] + 70, APP[1] - 14)
    end = (APPLICATIONS[0] - 70, APPLICATIONS[1] - 14)
    pts = bezier(start, ((start[0] + end[0]) / 2, APP[1] - 100), end)
    white = (255, 255, 255, 215)
    d.line([(x * ss, y * ss) for x, y in pts], fill=white, width=round(2.2 * ss), joint="curve")
    # Arrow head, aligned to the curve's end tangent.
    (x0, y0), (x1, y1) = pts[-4], pts[-1]
    ang = math.atan2(y1 - y0, x1 - x0)
    for da in (math.radians(150), math.radians(-150)):
        tip = (x1 + 11 * math.cos(ang + da), y1 + 11 * math.sin(ang + da))
        d.line([(x1 * ss, y1 * ss), (tip[0] * ss, tip[1] * ss)], fill=white, width=round(2.2 * ss))
    for x, y in (pts[0], pts[-1]):
        r0 = 1.1 * ss
        d.ellipse([x * ss - r0, y * ss - r0, x * ss + r0, y * ss + r0], fill=white)

    font = ImageFont.truetype(FONT, round(13 * ss))
    text = "Drag tern to Applications"
    tw = d.textlength(text, font=font)
    d.text(((w - tw) / 2, 44 * ss), text, font=font, fill=(255, 255, 255, 200))

    img.paste(layer, (0, 0), layer)
    img = img.resize((W * s, H * s), Image.LANCZOS)
    img.save(path)
    print(f"wrote {os.path.relpath(path, ROOT)} ({img.size[0]}x{img.size[1]})")


render(1, os.path.join(OUT, "dmg-background.png"))
render(2, os.path.join(OUT, "dmg-background@2x.png"))
