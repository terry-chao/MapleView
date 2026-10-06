"""Generate the MapleView application icon set from the logo bitmap.

The logo export is a portrait lockup -- a shaded mark above the characters
枫阅. An application icon has to survive 16x16, where the name turns to mud, so
the icon is the mark alone, centred on a rounded tile.

    python tools/gen_app_icon.py
    python tools/gen_app_icon.py --tile '#FFFFFF'
    python tools/gen_app_icon.py --tile none          # transparent backdrop

Output lands next to the logo:

    assets/logo/mapleview-icon-<size>.png     16 .. 1024
    assets/logo/mapleview-icon.ico            16/24/32/48/64/128/256

The mark is a rounded rectangle with a maple leaf breaking out over its
top-right corner. The rectangle is what reads as the body, so it -- not the
combined bounding box -- is what gets centred; the leaf is allowed to overhang
it. Centring the combined box instead would push the rectangle left and down.

The mark sits on a white background in the source, so it is keyed out by
whiteness before being composited onto the tile. The palest ink in the artwork
is around #FDD6B7 (darkest channel 183, a whiteness of 72), well clear of the
20 used for the key, which is what keeps the pale highlight opaque.
"""

import argparse
import os

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOGO_DIR = os.path.join(ROOT, "assets", "logo")
STEM = os.path.join(LOGO_DIR, "mapleview-icon")
# The watermark-free logo, then the export names it may still be filed under.
# Either way the watermark sits near y 1935..2009, far below the mark crop, so
# it never reaches the icon.
SOURCES = [
    os.path.join(LOGO_DIR, "mapleview-logo.png"),
    os.path.join(LOGO_DIR, "枫阅logo设计 (1).png"),
    os.path.join(LOGO_DIR, "枫阅logo设计 (1).jpeg"),
    os.path.join(LOGO_DIR, "枫阅logo设计.jpeg"),
]

# Measured from the source: the mark occupies rows 529..1162, cols 688..1442.
LEAF_BOX = (688, 529, 1443, 1163)

# ...and inside that crop the rounded rectangle spans x 1..673, y 99..634. The
# leaf reaches the crop's top edge (y 0) between x 436..642 and the right edge
# (x 755) between y 135..325, so it overhangs the rectangle up and to the right.
RECT_BOX = (1, 99, 673, 634)

MASTER = 1024          # nothing is ever upscaled: the mark is 755 px wide
LEAF_FRACTION = 0.72   # rectangle width as a fraction of the tile
EDGE_MARGIN = 0.03     # keep the overhanging leaf clear of the tile edge
CORNER = 0.22          # rounded-corner radius, as a fraction of the tile
KEY_LO, KEY_HI = 5, 25  # whiteness ramp that turns paper into transparency
PNG_SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)
TILE_TOP = (255, 244, 228)     # warm cream
TILE_BOTTOM = (246, 216, 171)


def keyed_leaf():
    """The mark crop as RGBA, with the white paper keyed out."""
    source = next((p for p in SOURCES if os.path.exists(p)), None)
    if source is None:
        raise SystemExit("no logo bitmap found in " + LOGO_DIR)
    crop = np.asarray(Image.open(source).convert("RGB").crop(LEAF_BOX)).astype(np.float32)
    whiteness = 255.0 - crop.min(axis=2)
    alpha = np.clip((whiteness - KEY_LO) / (KEY_HI - KEY_LO), 0.0, 1.0)[:, :, None]
    return crop, alpha


def tile(size, colour):
    """Rounded tile: vertical cream gradient, or fully transparent."""
    if colour == "none":
        return Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ramp = np.linspace(0.0, 1.0, size)[:, None, None]
    top, bottom = np.array(TILE_TOP, np.float32), np.array(TILE_BOTTOM, np.float32)
    if colour:
        top = bottom = np.array(colour, np.float32)
    body = top + (bottom - top) * ramp
    body = np.repeat(body, size, axis=1)
    # Supersample the rounded corners so small sizes stay clean.
    ss = 4
    mask = Image.new("L", (size * ss, size * ss), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size * ss - 1, size * ss - 1], radius=int(size * ss * CORNER), fill=255
    )
    mask = mask.resize((size, size), Image.LANCZOS)
    out = Image.fromarray(body.astype(np.uint8), "RGB").convert("RGBA")
    out.putalpha(mask)
    return out


def build(size):
    """One icon at `size` px: mark composited onto the tile."""
    leaf, alpha = keyed_leaf()
    height, width = leaf.shape[:2]
    rx0, ry0, rx1, ry1 = RECT_BOX
    cx, cy = (rx0 + rx1) / 2.0, (ry0 + ry1) / 2.0

    # Centre the rectangle, and let the leaf overhang it -- but never off the
    # tile, so cap the scale by how far the mark reaches in each direction.
    scale = LEAF_FRACTION * size / (rx1 - rx0)
    reach = max(cx, width - cx, cy, height - cy)
    scale = min(scale, (size / 2.0 - EDGE_MARGIN * size) / reach)

    out_size = (max(1, int(round(width * scale))), max(1, int(round(height * scale))))
    leaf_img = Image.fromarray(leaf.astype(np.uint8)).resize(out_size, Image.LANCZOS)
    alpha_img = Image.fromarray((alpha[:, :, 0] * 255).astype(np.uint8)).resize(
        leaf_img.size, Image.LANCZOS
    )
    leaf_arr = np.asarray(leaf_img).astype(np.float32)
    a = np.asarray(alpha_img).astype(np.float32)[:, :, None] / 255.0

    canvas = np.asarray(tile(size, TILE)).astype(np.float32)
    x = int(round(size / 2.0 - cx * scale))
    y = int(round(size / 2.0 - cy * scale))

    # Clip defensively: the scale cap keeps the mark inside, but a future crop
    # change should degrade to a clipped icon rather than a broken slice.
    src_x, src_y = max(0, -x), max(0, -y)
    dst_x, dst_y = max(0, x), max(0, y)
    w = min(leaf_img.width - src_x, size - dst_x)
    h = min(leaf_img.height - src_y, size - dst_y)
    if w > 0 and h > 0:
        region = canvas[dst_y:dst_y + h, dst_x:dst_x + w, :3]
        cover = a[src_y:src_y + h, src_x:src_x + w]
        canvas[dst_y:dst_y + h, dst_x:dst_x + w, :3] = (
            region * (1 - cover) + leaf_arr[src_y:src_y + h, src_x:src_x + w] * cover
        )
    return Image.fromarray(np.clip(canvas, 0, 255).astype(np.uint8), "RGBA")


def parse_colour(text):
    if text.lower() == "none":
        return "none"
    value = text.lstrip("#")
    if len(value) != 6:
        raise argparse.ArgumentTypeError("expected #RRGGBB or none")
    return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4))


def main():
    parser = argparse.ArgumentParser(description="Build the MapleView app icons.")
    parser.add_argument("--tile", type=parse_colour, default=None,
                        help="#RRGGBB for a flat tile, or none for transparency")
    args = parser.parse_args()

    global TILE
    TILE = args.tile
    os.makedirs(LOGO_DIR, exist_ok=True)
    for size in PNG_SIZES:
        build(size).save(f"{STEM}-{size}.png", "PNG", optimize=True)
    master = build(ICO_SIZES[-1])
    master.save(f"{STEM}.ico", format="ICO", sizes=[(s, s) for s in ICO_SIZES])
    print(f"tile            {'transparent' if args.tile == 'none' else args.tile or 'warm cream'}")
    print(f"png             {', '.join(str(s) for s in PNG_SIZES)}")
    print(f"ico             {', '.join(str(s) for s in ICO_SIZES)}")
    print(f"wrote           {os.path.relpath(STEM, ROOT)}.png / .ico")


if __name__ == "__main__":
    main()
