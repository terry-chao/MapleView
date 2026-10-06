"""Blank the AIGC watermark band off a logo export.

Doubao stamps generated images with a faint mark in the bottom-right corner.
On these exports it is a separate ink band, hundreds of rows below the artwork,
so it can be removed by clearing that band without touching a single lit pixel
of the logo itself.

    python tools/clean_logo.py
    python tools/clean_logo.py "assets/logo/枫阅logo设计 (1).jpeg" --out out.png

The band is only cleared when it looks like a watermark: it must be the last
ink band, sit near the bottom of the frame, be short, and be far fainter than
the artwork above it. Anything else is left alone and reported.
"""

import argparse
import os
import sys

import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_IN = os.path.join(ROOT, "assets", "logo", "枫阅logo设计 (1).jpeg")

INK_MAX = 238        # darkest channel below this counts as ink
GAP = 60             # blank rows that separate the artwork from a watermark
BOTTOM = 0.8         # the band must start below this fraction of the height
MAX_HEIGHT = 0.15    # and be shorter than this fraction
FAINT = 140          # and its darkest pixel must be at least this light


def ink_bands(gray):
    """Row runs of ink, splitting wherever there is a gap of GAP blank rows."""
    rows = np.nonzero((gray < INK_MAX).sum(axis=1))[0]
    if len(rows) == 0:
        return []
    bands, start, prev = [], rows[0], rows[0]
    for y in rows[1:]:
        if y - prev > GAP:
            bands.append((start, prev))
            start = y
        prev = y
    bands.append((start, prev))
    return bands


def find_watermark(img):
    """Row to clear from, or None."""
    bands = ink_bands(img.min(axis=2))
    if len(bands) < 2:
        return None
    top, bottom = bands[-1]
    height = img.shape[0]
    looks_like_one = (
        top > BOTTOM * height
        and (bottom - top) < MAX_HEIGHT * height
        and int(img[top:bottom + 1].min()) >= FAINT
    )
    return top - GAP // 2 if looks_like_one else None


def clean(path, out):
    img = np.asarray(Image.open(path).convert("RGB")).copy()
    cut = find_watermark(img)
    if cut is not None:
        img[cut:, :] = 255
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Image.fromarray(img).save(out)
    return cut


def main():
    parser = argparse.ArgumentParser(description="Strip the logo watermark band.")
    parser.add_argument("source", nargs="?", default=DEFAULT_IN)
    parser.add_argument("--out", default=None, help="default: source with .png")
    args = parser.parse_args()

    out = args.out or os.path.splitext(args.source)[0] + ".png"
    before = np.asarray(Image.open(args.source).convert("RGB"))
    cut = clean(args.source, out)

    after = np.asarray(Image.open(out).convert("RGB"))
    changed = np.abs(before.astype(np.int16) - after.astype(np.int16)).max(axis=2) > 0
    rows = np.nonzero(changed.sum(axis=1))[0]
    print(f"source    {os.path.relpath(args.source, ROOT)}  {before.shape[1]}x{before.shape[0]}")
    if cut is None:
        print("watermark none found - left untouched")
    else:
        print(f"watermark cleared from row {cut}")
        print(f"changed   {int(changed.sum())} px, rows {rows.min()}..{rows.max()} "
              "(the whole watermark band, nothing else)")
    print(f"wrote     {os.path.relpath(out, ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
