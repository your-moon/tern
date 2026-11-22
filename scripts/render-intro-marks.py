#!/usr/bin/env python3
"""Bake the Zeron intro's rotating color cards for GPUI.

Run with a zeron checkout at b289003b71b2072ac4170064bfdfb8d0b2dbf15b.
Requires Pillow and rsvg-convert. Geometry and colors are from intro.luau's
agent() and rise(). Each frame is one degree apart; intro_marks.rs selects it.
"""

import argparse
import io
import math
from pathlib import Path
import re
import subprocess

from PIL import Image, ImageDraw, ImageFilter


def render_card(source, size):
    card = Image.new("RGBA", (size, size))
    mask = Image.new("L", card.size)
    box = (0, 0, size - 1, size - 1)
    ImageDraw.Draw(mask).rounded_rectangle(box, size * 0.25, fill=255)
    draw = ImageDraw.Draw(card)
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2 * size - 2)
            draw.point((x, y), fill=(round(255 - 9*t), round(255 - 9*t), round(255 - 7*t), 255))
    card.putalpha(mask)
    draw.rounded_rectangle((1, 1, size-2, size-2), size * 0.25, outline=(255, 255, 255, 204), width=1)
    length = str(round(size * 0.56))
    png = subprocess.check_output(["rsvg-convert", "--width", length, "--height", length,
                                   "--keep-aspect-ratio", str(source)])
    icon = Image.open(io.BytesIO(png)).convert("RGBA")
    card.alpha_composite(icon, ((size - icon.width)//2, (size - icon.height)//2))
    return card


def atlas(card, rotation, wobble):
    side = card.width + 112
    angles = range(min(0, wobble), 25)
    sheet = Image.new("RGBA", (side * 6, side * math.ceil(len(angles) / 6)))
    for slot, angle in enumerate(angles):
        face = card.rotate(-(rotation + angle), Image.Resampling.BICUBIC, expand=True)
        x, y = (side - face.width)//2, (side - face.height)//2
        tile = Image.new("RGBA", (side, side))
        shadow = Image.new("RGBA", face.size, (26, 15, 58, 0))
        shadow.putalpha(face.getchannel("A").point(lambda alpha: alpha * 115 // 255))
        tile.alpha_composite(shadow, (x, y + 20))
        tile = tile.filter(ImageFilter.GaussianBlur(24))
        tile.alpha_composite(face, (x, y))
        sheet.paste(tile, (slot % 6 * side, slot // 6 * side))
    return sheet


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("zeron", type=Path)
    ui = parser.parse_args().zeron / "apps/zgx/ui"
    output = Path(__file__).resolve().parents[1] / "crates/tern/assets/intro/marks"
    output.mkdir(parents=True, exist_ok=True)
    agents = (ui / "intro.luau").read_text().split("local AGENTS: { Agent } = {")[1]
    for block in re.findall(r"\{([^{}]+)\}", agents.split("-- When the agents")[0]):
        values = dict(re.findall(r'(\w+) = ("[^"]+"|[-.\d]+)', block))
        name = values["mark"].strip('"')
        card = render_card(ui / "assets/icons" / (name + ".svg"), int(values["s"]))
        sheet = atlas(card, int(values["rot"]), int(values["wob"]))
        sheet.save(output / (name + ".webp"), lossless=True, method=1)


if __name__ == "__main__":
    main()
