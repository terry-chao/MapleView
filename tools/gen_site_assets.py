"""Generate the procedural image corpus and brand assets used by the website.

Everything here is synthetic: no third-party photos, no network access, and the
output is deterministic for a given seed so the site can be rebuilt byte-for-byte.

Run from the repository root:

    python tools/gen_site_assets.py

It writes:

    docs/assets/demo/*            the demo corpus (mixed formats, ~6 MP each)
    docs/assets/javascripts/demo-corpus.js   the manifest the browser reads
    docs/assets/img/*             favicon / social card / logo copy
"""

from __future__ import annotations

import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEMO_DIR = os.path.join(ROOT, "docs", "assets", "demo")
JS_DIR = os.path.join(ROOT, "docs", "assets", "javascripts")
IMG_DIR = os.path.join(ROOT, "docs", "assets", "brand")
# The desktop app's icon set; the site assets are sampled from it.
ICON_PNG = os.path.join(ROOT, "assets", "logo", "mapleview-icon-512.png")

HERO_W, HERO_H = 3000, 2000  # 6 MP, the size the README benchmarks against.


# --------------------------------------------------------------------------- #
# noise helpers
# --------------------------------------------------------------------------- #
def _value_noise(h: int, w: int, res: int, rng: np.random.Generator, smooth: bool) -> np.ndarray:
    grid = rng.random((res + 1, res + 1)).astype(np.float32)
    resample = Image.Resampling.BICUBIC if smooth else Image.Resampling.NEAREST
    small = Image.fromarray((grid * 255).astype(np.uint8), mode="L")
    up = small.resize((w, h), resample)
    return np.asarray(up, dtype=np.float32) / 255.0


def fbm(
    h: int,
    w: int,
    octaves: int = 6,
    seed: int = 0,
    res: int = 3,
    gain: float = 0.5,
    smooth: bool = True,
) -> np.ndarray:
    """Fractal value noise in [0, 1]."""
    rng = np.random.default_rng(seed)
    out = np.zeros((h, w), np.float32)
    amp, total = 1.0, 0.0
    for o in range(octaves):
        out += amp * _value_noise(h, w, max(2, res * (2**o)), rng, smooth)
        total += amp
        amp *= gain
    return out / total


def ramp(h: int, stops: list[tuple[float, tuple[float, float, float]]]) -> np.ndarray:
    """Vertical colour ramp -> (h, 3) float array."""
    ys = np.linspace(0.0, 1.0, h, dtype=np.float32)
    pos = np.array([s[0] for s in stops], dtype=np.float32)
    lut = np.zeros((h, 3), np.float32)
    for ch in range(3):
        vals = np.array([s[1][ch] for s in stops], dtype=np.float32)
        lut[:, ch] = np.interp(ys, pos, vals)
    return lut


def radial(h: int, w: int, cx: float, cy: float, radius: float, power: float = 2.0) -> np.ndarray:
    ys = np.arange(h, dtype=np.float32)[:, None] - cy * h
    xs = np.arange(w, dtype=np.float32)[None, :] - cx * w
    d = np.sqrt(xs * xs + ys * ys) / (radius * max(w, h))
    return np.clip(1.0 - d, 0.0, 1.0) ** power


def sprites(
    h: int,
    w: int,
    count: int,
    seed: int,
    r_min: float,
    r_max: float,
    tint: tuple[float, float, float],
    downscale: int = 3,
    jitter: float = 0.6,
) -> np.ndarray:
    """Soft light blobs, drawn tiny and upscaled so 900 of them stay cheap."""
    sw, sh = max(8, w // downscale), max(8, h // downscale)
    canvas = Image.new("RGB", (sw, sh), (0, 0, 0))
    draw = ImageDraw.Draw(canvas)
    rng = np.random.default_rng(seed)
    for _ in range(count):
        cx, cy = rng.random() * sw, rng.random() * sh
        r = (r_min + rng.random() * (r_max - r_min)) * sw
        k = 0.35 + jitter * rng.random()
        col = tuple(int(np.clip(c * k * 255, 0, 255)) for c in tint)
        draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=col)
    big = canvas.resize((w, h), Image.Resampling.BICUBIC)
    return np.asarray(big, dtype=np.float32) / 255.0


def vignette(h: int, w: int, strength: float = 0.35) -> np.ndarray:
    ys = np.linspace(0.0, 1.0, h, dtype=np.float32)[:, None] - 0.5
    xs = np.linspace(0.0, 1.0, w, dtype=np.float32)[None, :] - 0.5
    d = np.sqrt(xs * xs + ys * ys)
    return np.clip(1.0 - strength * (d / 0.71) ** 2.2, 0.0, 1.0)


def grain(h: int, w: int, amount: float, seed: int) -> np.ndarray:
    rng = np.random.default_rng(seed)
    return rng.normal(0.0, amount, (h, w, 1)).astype(np.float32)


def finish(img: np.ndarray, h: int, w: int, vig: float = 0.32, g: float = 0.006, seed: int = 7) -> np.ndarray:
    img = img * vignette(h, w, vig)[:, :, None]
    img = img + grain(h, w, g, seed)
    return np.clip(img, 0.0, 1.0)


def to_pil(img: np.ndarray) -> Image.Image:
    return Image.fromarray((np.clip(img, 0, 1) * 255.0 + 0.5).astype(np.uint8), mode="RGB")


# --------------------------------------------------------------------------- #
# the pictures
# --------------------------------------------------------------------------- #
def img_aurora(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.02, 0.03, 0.09)), (0.55, (0.03, 0.06, 0.14)), (1.0, (0.01, 0.02, 0.05))])
    img = np.repeat(base[:, None, :], w, axis=1)
    band = fbm(h, w, octaves=5, seed=seed, res=2)
    streak = np.exp(-(((np.linspace(0, 1, h)[:, None] - 0.42 - 0.22 * band) * 7.0) ** 2))
    glow = streak * (0.35 + 0.65 * fbm(h, w, octaves=6, seed=seed + 11, res=3))
    green = glow[:, :, None] * np.array([0.15, 0.95, 0.62], np.float32)
    img += green * 0.85
    curtain = np.exp(-(((np.linspace(0, 1, h)[:, None] - 0.30 - 0.3 * band) * 11.0) ** 2))
    img += curtain[:, :, None] * np.array([0.45, 0.25, 0.85], np.float32) * 0.45
    img += radial(h, w, 0.78, 0.12, 0.5, 1.6)[:, :, None] * np.array([1.0, 0.95, 0.85], np.float32) * 0.20
    ridge = fbm(h, w, octaves=5, seed=seed + 5, res=3)
    horizon = 0.80 + 0.05 * ridge
    mask = (np.linspace(0, 1, h)[:, None] > horizon).astype(np.float32)
    img = img * (1 - mask[:, :, None]) + mask[:, :, None] * np.array([0.02, 0.03, 0.06], np.float32)
    return finish(img, h, w, vig=0.4, seed=seed)


def img_sunset(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(
        h,
        [
            (0.0, (0.10, 0.16, 0.38)),
            (0.35, (0.62, 0.34, 0.42)),
            (0.55, (0.97, 0.55, 0.26)),
            (0.70, (0.99, 0.78, 0.38)),
            (1.0, (0.30, 0.14, 0.16)),
        ],
    )
    img = np.repeat(base[:, None, :], w, axis=1)
    sun = radial(h, w, 0.62, 0.58, 0.45, 3.0)
    img += sun[:, :, None] * np.array([1.0, 0.85, 0.55], np.float32) * 0.8
    disc = radial(h, w, 0.62, 0.58, 0.16, 8.0)
    img += disc[:, :, None] * np.array([1.0, 0.96, 0.85], np.float32)
    cloud = fbm(h, w, octaves=6, seed=seed, res=3)
    band = np.exp(-(((np.linspace(0, 1, h)[:, None] - 0.45) * 5.0) ** 2))
    img += (cloud * band)[:, :, None] * np.array([1.0, 0.6, 0.4], np.float32) * 0.28
    ridge = fbm(h, w, octaves=4, seed=seed + 3, res=4)
    for i, (lvl, col) in enumerate([(0.72, (0.16, 0.09, 0.15)), (0.82, (0.09, 0.05, 0.10))]):
        line = lvl + 0.05 * fbm(h, w, octaves=3, seed=seed + 20 + i, res=5)
        m = (np.linspace(0, 1, h)[:, None] > line).astype(np.float32)
        img = img * (1 - m[:, :, None]) + m[:, :, None] * np.array(col, np.float32)
    return finish(img, h, w, seed=seed)


def img_forest(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.05, 0.16, 0.11)), (0.5, (0.04, 0.11, 0.08)), (1.0, (0.02, 0.05, 0.04))])
    img = np.repeat(base[:, None, :], w, axis=1)
    shafts = fbm(h, w, octaves=4, seed=seed, res=2)
    m = np.exp(-(((np.linspace(0, 1, w)[None, :] - 0.3 - 0.5 * shafts) * 6.0) ** 2))
    fall = np.clip(1.0 - np.linspace(0, 1, h)[:, None] * 0.9, 0, 1)
    img += (m * fall)[:, :, None] * np.array([0.85, 0.95, 0.55], np.float32) * 0.55
    canopy = fbm(h, w, octaves=7, seed=seed + 2, res=3)
    top = np.clip(1.0 - np.linspace(0, 1, h)[:, None] * 2.2, 0, 1)
    img *= (1.0 - 0.55 * canopy * top)[:, :, None]
    trunks = fbm(h, w, octaves=2, seed=seed + 9, res=40, smooth=False)
    img *= (0.55 + 0.45 * trunks)[:, :, None]
    haze = radial(h, w, 0.42, 0.35, 0.9, 1.2)
    img += haze[:, :, None] * np.array([0.5, 0.7, 0.5], np.float32) * 0.10
    return finish(img, h, w, vig=0.45, seed=seed)


def img_dunes(h: int, w: int, seed: int) -> np.ndarray:
    yy = np.linspace(0, 1, h, dtype=np.float32)[:, None]
    xx = np.linspace(0, 1, w, dtype=np.float32)[None, :]
    img = np.zeros((h, w, 3), np.float32)
    for i in range(9):
        n = fbm(h, w, octaves=3, seed=seed + i * 7, res=3)
        ridge = i / 9.0 + 0.06 * (n - 0.5)
        m = (yy > ridge).astype(np.float32)
        tone = 0.42 + 0.5 * (i / 9.0) + 0.12 * (0.5 - np.abs(xx - 0.45))
        col = np.stack([tone * 1.15, tone * 0.86, tone * 0.56], axis=-1)
        shade = 1.0 - 0.5 * np.clip(n - 0.5, 0, 1) * 4
        img = img * (1 - m[:, :, None]) + (m[:, :, None] * col * shade[:, :, None])
    sky = ramp(h, [(0.0, (0.35, 0.55, 0.78)), (0.5, (0.80, 0.85, 0.86))])
    topm = (yy < 0.22).astype(np.float32)
    img = img * (1 - topm[:, :, None]) + topm[:, :, None] * np.repeat(sky[:, None, :], w, axis=1)
    img += radial(h, w, 0.2, 0.1, 0.6, 1.5)[:, :, None] * np.array([1.0, 0.9, 0.7], np.float32) * 0.25
    return finish(img, h, w, seed=seed)


def img_ocean(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.09, 0.42, 0.58)), (0.5, (0.03, 0.18, 0.38)), (1.0, (0.01, 0.05, 0.15))])
    img = np.repeat(base[:, None, :], w, axis=1)
    img += 0.40 * sprites(h, w, 110, seed, 0.004, 0.055, (0.45, 0.85, 1.0), downscale=3)
    caustics = np.clip(fbm(h, w, octaves=6, seed=seed + 4, res=2) - 0.55, 0, 1) * 2.2
    img += caustics[:, :, None] * np.array([0.6, 0.95, 1.0], np.float32) * 0.35
    beam = np.exp(-(((np.linspace(0, 1, w)[None, :] - 0.62) * 5.0) ** 2))
    img += beam[:, :, None] * np.array([0.6, 0.85, 1.0], np.float32) * 0.30
    return finish(img, h, w, vig=0.45, seed=seed)


def img_neon(h: int, w: int, seed: int) -> np.ndarray:
    img = np.zeros((h, w, 3), np.float32)
    img += ramp(h, [(0.0, (0.03, 0.02, 0.08)), (1.0, (0.10, 0.02, 0.16))])[:, None, :]
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    hz = np.exp(-(((yy / h - 0.62)) * 12.0) ** 2)
    for i, col in enumerate([(1.0, 0.20, 0.55), (0.25, 0.85, 1.0), (1.0, 0.75, 0.2)]):
        off = (i - 1) * 0.11
        line = np.exp(-(((xx / w - 0.5 - off)) * 26.0) ** 2)
        img += (line * hz)[:, :, None] * np.array(col, np.float32) * 0.9
    grid = np.exp(-(((xx / w * 34.0) % 1.0 - 0.5) * 7.0) ** 2)
    horizon = np.clip((yy / h - 0.62) * 4.0, 0, 1)
    img += (grid * horizon)[:, :, None] * np.array([0.35, 0.9, 1.0], np.float32) * 0.35
    for i in range(12):
        bx = (i + 0.5) / 12.0
        bw = 0.012 + 0.02 * ((i * 37) % 7) / 7.0
        bh = 0.10 + 0.34 * ((i * 53) % 11) / 11.0
        m = ((np.abs(xx / w - bx) < bw / 2) & (yy / h > 0.62 - bh) & (yy / h < 0.62)).astype(np.float32)
        tint = np.array([0.6, 1.0, 0.95], np.float32) if i % 2 else np.array([1.0, 0.5, 0.9], np.float32)
        img += m[:, :, None] * tint * 0.33
    img += radial(h, w, 0.5, 0.6, 0.5, 1.4)[:, :, None] * np.array([0.6, 0.3, 1.0], np.float32) * 0.35
    return finish(img, h, w, vig=0.5, seed=seed)


def img_maple(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.95, 0.72, 0.42)), (0.45, (0.86, 0.42, 0.18)), (1.0, (0.38, 0.10, 0.08))])
    img = np.repeat(base[:, None, :], w, axis=1)
    img += 0.9 * sprites(h, w, 150, seed, 0.010, 0.075, (1.0, 0.52, 0.22), downscale=3)
    img += radial(h, w, 0.25, 0.2, 0.7, 1.4)[:, :, None] * np.array([1.0, 0.8, 0.5], np.float32) * 0.40
    yy = np.linspace(0, 1, h, dtype=np.float32)[:, None]
    xx = np.linspace(0, 1, w, dtype=np.float32)[None, :]
    veins = np.abs(np.sin((xx * 9.0 + 0.6 * np.sin(yy * 5.0)) * np.pi))
    img *= (0.86 + 0.14 * veins)[:, :, None]
    return finish(img, h, w, vig=0.42, seed=seed)


def img_mist(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.62, 0.70, 0.78)), (0.45, (0.86, 0.88, 0.88)), (1.0, (0.96, 0.95, 0.92))])
    img = np.repeat(base[:, None, :], w, axis=1)
    for i in range(5):
        n = fbm(h, w, octaves=4, seed=seed + i * 13, res=3)
        lvl = 0.34 + i * 0.13 + 0.05 * (n - 0.5)
        m = (np.linspace(0, 1, h)[:, None] > lvl).astype(np.float32)
        tone = 0.52 + 0.30 * i / 5.0
        col = np.stack([tone * 0.94, tone * 0.97, tone], axis=-1)
        fog = np.clip(1.0 - np.abs(np.linspace(0, 1, h)[:, None] - lvl) * 6.0, 0, 1)
        col = col + fog[:, :, None] * 0.35
        img = img * (1 - m[:, :, None]) + m[:, :, None] * col
    sun = radial(h, w, 0.68, 0.22, 0.5, 2.0)
    img += sun[:, :, None] * np.array([1.0, 0.96, 0.82], np.float32) * 0.35
    return finish(img, h, w, vig=0.25, seed=seed)


def img_violet(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.30, 0.16, 0.50)), (0.5, (0.16, 0.08, 0.30)), (1.0, (0.06, 0.03, 0.14))])
    img = np.repeat(base[:, None, :], w, axis=1)
    yy = np.arange(h, dtype=np.float32)[:, None]
    xx = np.arange(w, dtype=np.float32)[None, :]
    petals = 16
    ang = np.arctan2(yy / h - 0.5, xx / w - 0.5)
    rad = np.sqrt((xx / w - 0.5) ** 2 + (yy / h - 0.5) ** 2)
    fold = 0.5 + 0.5 * np.cos(ang * petals)
    r = 0.10 + 0.30 * fold + 0.06 * fbm(h, w, octaves=4, seed=seed, res=3)
    m = (rad < r).astype(np.float32)
    edge = np.clip(1.0 - np.abs(rad - r) * 26.0, 0, 1)
    img += m[:, :, None] * (np.array([0.72, 0.42, 0.95], np.float32) * 0.55)[None, None, :]
    img += edge[:, :, None] * np.array([0.95, 0.80, 1.0], np.float32) * 0.75
    img += radial(h, w, 0.5, 0.5, 0.18, 2.4)[:, :, None] * np.array([1.0, 0.95, 0.7], np.float32) * 0.9
    img += 0.7 * sprites(h, w, 80, seed + 6, 0.010, 0.050, (0.75, 0.55, 1.0), downscale=3)
    return finish(img, h, w, vig=0.5, seed=seed)


def img_stars(h: int, w: int, seed: int) -> np.ndarray:
    img = ramp(h, [(0.0, (0.02, 0.03, 0.08)), (0.6, (0.05, 0.06, 0.14)), (1.0, (0.02, 0.02, 0.05))])
    img = np.repeat(img[:, None, :], w, axis=1)
    band = np.exp(-(((np.linspace(0, 1, h)[:, None] - 0.45)) * 3.2) ** 2)
    img += (band * fbm(h, w, octaves=6, seed=seed, res=3))[:, :, None] * np.array([0.55, 0.55, 0.85], np.float32) * 0.7
    img += 1.7 * sprites(h, w, 1100, seed, 0.0007, 0.0032, (1.0, 1.0, 1.0), downscale=2, jitter=0.9)
    img += radial(h, w, 0.85, 0.85, 0.5, 1.2)[:, :, None] * np.array([0.7, 0.6, 0.5], np.float32) * 0.25
    for i in range(6):
        n = fbm(h, w, octaves=3, seed=seed + 30 + i, res=3)
        lvl = 0.72 + i * 0.05 + 0.06 * (n - 0.5)
        m = (np.linspace(0, 1, h)[:, None] > lvl).astype(np.float32)
        img = img * (1 - m[:, :, None]) + m[:, :, None] * np.array([0.02, 0.03, 0.05], np.float32)
    return finish(img, h, w, vig=0.35, seed=seed)


def img_snow(h: int, w: int, seed: int) -> np.ndarray:
    base = ramp(h, [(0.0, (0.36, 0.52, 0.76)), (0.4, (0.72, 0.82, 0.92)), (1.0, (0.92, 0.94, 0.97))])
    img = np.repeat(base[:, None, :], w, axis=1)
    yy = np.linspace(0, 1, h, dtype=np.float32)[:, None]
    for i in range(7):
        n = fbm(h, w, octaves=3, seed=seed + i * 5, res=3)
        lvl = 0.30 + i * 0.10 + 0.05 * (n - 0.5)
        m = (yy > lvl).astype(np.float32)
        tone = 0.90 - 0.10 * i / 7.0 + 0.06 * (n - 0.5) * 2
        img = img * (1 - m[:, :, None]) + m[:, :, None] * np.stack([tone * 1.0, tone * 1.02, tone * 1.06], -1)
    img += radial(h, w, 0.30, 0.18, 0.6, 1.6)[:, :, None] * np.array([1.0, 0.92, 0.75], np.float32) * 0.30
    return finish(img, h, w, vig=0.22, seed=seed)


def img_minimal(h: int, w: int, seed: int) -> np.ndarray:
    img = np.zeros((h, w, 3), np.float32)
    img += ramp(h, [(0.0, (0.98, 0.95, 0.89)), (1.0, (0.90, 0.84, 0.76))])[:, None, :]
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    cx, cy = 0.5, 0.46
    d = np.sqrt((xx / w - cx) ** 2 + ((yy / h - cy) * (h / w)) ** 2)
    shapes = [
        (0.30, (0.90, 0.36, 0.18)),
        (0.24, (0.97, 0.70, 0.29)),
        (0.18, (0.13, 0.45, 0.55)),
        (0.12, (0.24, 0.22, 0.28)),
        (0.06, (0.98, 0.96, 0.92)),
    ]
    for r, col in shapes:
        m = (d < r).astype(np.float32)
        img = img * (1 - m[:, :, None]) + m[:, :, None] * np.array(col, np.float32)
    bar = (np.abs(yy / h - 0.90) < 0.012).astype(np.float32)
    img = img * (1 - bar[:, :, None]) + bar[:, :, None] * np.array([0.20, 0.18, 0.22], np.float32)
    # 不加颗粒：PNG 要留着给「无损格式」当样本，噪点会让它胀到几 MB。
    return finish(img, h, w, vig=0.10, g=0.0, seed=seed)


# --------------------------------------------------------------------------- #
# corpus definition + writing
# --------------------------------------------------------------------------- #
CORPUS = [
    ("01-aurora", "极光", "JPEG", img_aurora, dict(quality=80)),
    ("02-sunset", "落日山脊", "JPEG", img_sunset, dict(quality=80)),
    ("03-forest", "林间光柱", "JPEG", img_forest, dict(quality=80)),
    ("04-dunes", "沙丘", "JPEG", img_dunes, dict(quality=80)),
    ("05-ocean", "深海微光", "JPEG", img_ocean, dict(quality=80)),
    ("06-neon", "霓虹网格", "JPEG", img_neon, dict(quality=80)),
    ("07-maple", "枫叶散景", "JPEG", img_maple, dict(quality=82)),
    ("08-mist", "晨雾层峦", "JPEG", img_mist, dict(quality=80)),
    ("09-violet", "紫罗兰", "JPEG", img_violet, dict(quality=80)),
    ("10-stars", "星野", "JPEG", img_stars, dict(quality=80)),
    ("11-minimal", "极简几何", "PNG", img_minimal, dict(optimize=True)),
    ("12-snow", "雪原", "WebP", img_snow, dict(quality=80, method=5)),
]


def fmt_bytes(n: int) -> str:
    if n >= 1024 * 1024:
        return f"{n / 1024 / 1024:.1f} MB"
    return f"{n / 1024:.0f} KB"


def brand_icon(size: int) -> Image.Image:
    """The app icon, downsampled from the bitmap the desktop build ships.

    Keeping one source of truth means the favicon, the touch icon and the social
    card can never drift away from the .exe icon again.
    """
    with Image.open(ICON_PNG) as source:
        return source.convert("RGBA").resize((size, size), Image.LANCZOS)


def _font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    for name in ("segoeuib.ttf", "seguisb.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"):
        for root in (r"C:\Windows\Fonts", "/usr/share/fonts/truetype/dejavu"):
            candidate = os.path.join(root, name)
            if os.path.exists(candidate):
                try:
                    return ImageFont.truetype(candidate, size)
                except OSError:
                    pass
    return ImageFont.load_default(size)


def social_card(path: str) -> None:
    w, h = 1200, 630
    bg = img_aurora(h, w, seed=4242)
    card = to_pil(bg).convert("RGB")
    overlay = Image.new("RGB", (w, h), (10, 9, 14))
    card = Image.blend(card, overlay, 0.55)
    d = ImageDraw.Draw(card)
    icon = brand_icon(160)
    card.paste(icon, (72, 72), icon)
    d.text((78, 292), "MapleView", font=_font(88), fill=(255, 255, 255))
    d.text((82, 402), "快如闪电的图片预览  ·  Rust + wgpu", font=_font(36), fill=(247, 178, 75))
    # 1200×630 的社交卡片用 JPEG：PNG 会有半 MB 以上，没必要。
    card.save(path, "JPEG", quality=88, optimize=True, progressive=True)


def main() -> int:
    brand_only = "--brand-only" in sys.argv[1:]
    only = None
    for arg in sys.argv[1:]:
        if arg.startswith("--only="):
            only = arg.split("=", 1)[1]
    os.makedirs(DEMO_DIR, exist_ok=True)
    os.makedirs(JS_DIR, exist_ok=True)
    os.makedirs(IMG_DIR, exist_ok=True)

    manifest: list[dict] = []
    total = 0
    if not brand_only:
        for idx, (slug, title, fmt, fn, opts) in enumerate(CORPUS):
            if only and slug != only:
                continue
            # 生成器签名是 (h, w)，别写反了 —— 反了会得到 2000×3000 的竖图，
            # 而 manifest 里写着 3000×2000，预览缩放就会把画压扁。
            img = fn(HERO_H, HERO_W, seed=1000 + idx * 17)
            pil = to_pil(img)
            ext = {"JPEG": "jpg", "PNG": "png", "WebP": "webp"}[fmt]
            name = f"{slug}.{ext}"
            path = os.path.join(DEMO_DIR, name)
            save_opts = dict(opts)
            if fmt == "JPEG":
                save_opts.update(subsampling=1, optimize=True, progressive=False)
            if fmt == "PNG":
                save_opts.update(optimize=True, compress_level=9)
            if fmt == "WebP":
                save_opts.update(method=6)
            pil.save(path, fmt, **save_opts)
            size = os.path.getsize(path)
            total += size
            manifest.append(
                {"file": name, "title": title, "format": fmt, "w": HERO_W, "h": HERO_H, "bytes": size}
            )
            print(f"  {name:22s} {fmt:5s} {fmt_bytes(size):>9s}", flush=True)

        if only:
            print("只重生成了单张图，manifest 未改动（跑完整流程才会重写 demo-corpus.js）")
            return 0

        manifest_js = (
            "// Generated by tools/gen_site_assets.py -- do not edit by hand.\n"
            "window.MV_CORPUS = "
            + json.dumps(manifest, ensure_ascii=False, indent=2)
            + ";\n"
        )
        with open(os.path.join(JS_DIR, "demo-corpus.js"), "w", encoding="utf-8", newline="\n") as fh:
            fh.write(manifest_js)
        print(f"\ncorpus total: {fmt_bytes(total)} across {len(manifest)} images")

    # favicon + logo copy + social card
    for size in (32, 48, 180, 512):
        brand_icon(size).save(os.path.join(IMG_DIR, f"icon-{size}.png"))
    brand_icon(64).save(os.path.join(IMG_DIR, "favicon.ico"), sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    social_card(os.path.join(IMG_DIR, "social-card.jpg"))
    print("wrote brand icons + social card")
    return 0


if __name__ == "__main__":
    sys.exit(main())
