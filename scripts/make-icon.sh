#!/usr/bin/env bash
# Regenerates packaging/macos/tern.icns from packaging/macos/icon.svg with tools that ship with
# macOS (qlmanage, sips, iconutil) plus Pillow for the transparent corners: Quick Look renders
# an SVG on an opaque white page, so the tile's rounded-square outline is cut out afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

qlmanage -t -s 1024 -o "$work" packaging/macos/icon.svg >/dev/null 2>&1
python3 - "$work/icon.svg.png" "$work/master.png" <<'PY'
import sys
from PIL import Image, ImageDraw

src, dst = sys.argv[1:3]
im = Image.open(src).convert("RGBA")
# The tile is x/y 100..924 with rx 185 in icon.svg; 4x supersampling smooths the edge.
mask = Image.new("L", (4096, 4096), 0)
ImageDraw.Draw(mask).rounded_rectangle((400, 400, 3696, 3696), radius=740, fill=255)
im.putalpha(mask.resize((1024, 1024), Image.LANCZOS))
im.save(dst)
PY

set_dir="$work/tern.iconset"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
  sips -z $s $s "$work/master.png" --out "$set_dir/icon_${s}x${s}.png" >/dev/null
  sips -z $((s * 2)) $((s * 2)) "$work/master.png" --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
cp "$work/master.png" "$set_dir/icon_512x512@2x.png"
iconutil -c icns "$set_dir" -o packaging/macos/tern.icns
echo "wrote packaging/macos/tern.icns"
