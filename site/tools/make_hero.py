"""Hero art for the landing page: tern's Great Wave wallpaper, dissolving into the page
background through the Bayer pattern of tern's dither effect (crates/tern/src/wallpaper_fx.rs)."""
import sys
from PIL import Image, ImageEnhance

BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]
BG = (6, 4, 10)  # --bg in site/style.css

src, out, W, H = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
im = Image.open(src).convert("RGB")
# Cover-crop to W:H, keeping the wave crest (upper-left third of the print) in frame.
s = max(W / im.width, H / im.height)
im = im.resize((round(im.width * s), round(im.height * s)), Image.LANCZOS)
left = (im.width - W) // 2
top = int((im.height - H) * 0.30)
im = im.crop((left, top, left + W, top + H))
im = ImageEnhance.Brightness(im).enhance(0.8)
px = im.load()

CELL = 2                     # tern's dither works on 2x2 cells
solid_until = 0.42           # above this the picture is whole
gone_at = 0.92               # below this only the background remains
for y in range(0, H, CELL):
    t = y / H
    keep = 1.0 if t <= solid_until else max(0.0, 1 - (t - solid_until) / (gone_at - solid_until))
    for x in range(0, W, CELL):
        thr = (BAYER[(y // CELL) % 4][(x // CELL) % 4] + 0.5) / 16
        if keep >= 1.0:
            continue
        r, g, b = px[min(x + 1, W - 1), min(y + 1, H - 1)]
        if thr < keep:
            # A surviving dot keeps the painting's colour, so the fade has no seam.
            c = (r, g, b)
        else:
            c = BG
        for dy in range(CELL):
            for dx in range(CELL):
                if x + dx < W and y + dy < H:
                    px[x + dx, y + dy] = c
im.save(out, "JPEG", quality=84, optimize=True, progressive=True)
print(out, im.size)
