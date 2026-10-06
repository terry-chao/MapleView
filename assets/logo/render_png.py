"""Regenerate the raster logo exports next to the SVGs.

The leaf outline is read straight out of ``mapleview-mark.svg`` so the vector
and raster versions can never drift apart. Requires Pillow:

    python assets/logo/render_png.py
"""

import os
import re

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))

# Mirrors the gradients in the SVG files.
LEAF_STOPS = [(0.0, (249, 190, 92)), (0.5, (228, 87, 46)), (1.0, (175, 42, 34))]
ICON_BG_STOPS = [(0.0, (58, 32, 26)), (1.0, (30, 18, 14))]
INK = (43, 29, 22)
AMBER = (181, 112, 58)

SS = 4  # supersampling factor


def leaf_points():
    svg = open(os.path.join(HERE, "mapleview-mark.svg"), encoding="utf-8").read()
    d = re.search(r'\sd="([^"]+)"', svg).group(1)
    nums = [float(n) for n in re.findall(r"-?\d+(?:\.\d+)?", d)]
    return list(zip(nums[0::2], nums[1::2]))


LEAF = leaf_points()
CX, CY = (
    (min(p[0] for p in LEAF) + max(p[0] for p in LEAF)) / 2,
    (min(p[1] for p in LEAF) + max(p[1] for p in LEAF)) / 2,
)
HALF_W = (max(p[0] for p in LEAF) - min(p[0] for p in LEAF)) / 2
HALF_H = (max(p[1] for p in LEAF) - min(p[1] for p in LEAF)) / 2


def _lerp(a, b, t):
    return tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))


def gradient_rows(width, height, y0, y1, stops):
    """Vertical gradient spanning [y0, y1] in pixels, clamped outside."""
    img = Image.new("RGB", (width, height))
    px = img.load()
    span = max(1.0, y1 - y0)
    for y in range(height):
        t = min(1.0, max(0.0, (y - y0) / span))
        color = stops[-1][1]
        for i in range(len(stops) - 1):
            o0, c0 = stops[i]
            o1, c1 = stops[i + 1]
            if o0 <= t <= o1:
                local = 0.0 if o1 == o0 else (t - o0) / (o1 - o0)
                color = _lerp(c0, c1, local)
                break
        for x in range(width):
            px[x, y] = color
    return img


def leaf_layer(width, height, center, scale, stops):
    big = (width * SS, height * SS)
    mask = Image.new("L", big, 0)
    cx, cy = center
    pts = []
    for x, y in LEAF:
        pts.append((((x - CX) * scale + cx) * SS, ((y - CY) * scale + cy) * SS))
    ImageDraw.Draw(mask).polygon(pts, fill=255)
    mask = mask.resize((width, height), Image.LANCZOS)

    grad = gradient_rows(width, height, center[1] - HALF_H * scale, center[1] + HALF_H * scale, stops)
    layer = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    layer.paste(grad, (0, 0), mask)
    return layer


def rounded_rect_mask(size, radius):
    big = size * SS
    m = Image.new("L", (big, big), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, big - 1, big - 1], radius=radius * SS, fill=255)
    return m.resize((size, size), Image.LANCZOS)


def save(img, name):
    path = os.path.join(HERE, name)
    img.save(path, "PNG")
    print(f"wrote {path}  {img.size[0]}x{img.size[1]}")


def render_mark(size):
    return leaf_layer(size, size, (size / 2, size / 2), size / 100.0 * 0.92, LEAF_STOPS)


def render_icon(size):
    bg = gradient_rows(size, size, 0, size, ICON_BG_STOPS)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(bg, (0, 0), rounded_rect_mask(size, size * 0.219))
    leaf = leaf_layer(size, size, (size / 2, size / 2 + size * 0.012), size / 100.0 * 0.745, LEAF_STOPS)
    return Image.alpha_composite(out, leaf)


def font_path(names):
    for name in names:
        path = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts", name)
        if os.path.exists(path):
            return path
    return None


def load_cn_font(px):
    path = font_path(("msyhbd.ttc", "msyh.ttc", "Dengb.ttf", "simhei.ttf"))
    return ImageFont.truetype(path, px) if path else ImageFont.load_default()


def load_latin_font(px):
    path = font_path(("segoeuisb.ttf", "segoeuib.ttf", "arialbd.ttf"))
    return ImageFont.truetype(path, px) if path else load_cn_font(px)


def text_ink(text, font, fill, spacing=0):
    """Render text and return (ink box image, baseline y within that image)."""
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    widths = [probe.textlength(ch, font=font) for ch in text]
    width = int(sum(widths) + spacing * max(0, len(text) - 1)) + 8
    ascent, descent = font.getmetrics()
    img = Image.new("RGBA", (width, ascent + descent + 8), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    baseline = 4 + ascent
    x = 4.0
    for ch, cw in zip(text, widths):
        d.text((x, baseline), ch, font=font, fill=fill, anchor="ls")
        x += cw + spacing
    box = img.getbbox() or (0, 0, 0, 0)
    return img.crop(box), baseline - box[1]


def pad_canvas(img, pad):
    out = Image.new("RGBA", (img.size[0] + 2 * pad, img.size[1] + 2 * pad), (0, 0, 0, 0))
    out.paste(img, (pad, pad))
    return out


def render_lockup():
    mark_scale = 3.6
    mark = leaf_layer(
        int(round(2 * HALF_W * mark_scale)),
        int(round(2 * HALF_H * mark_scale)),
        (HALF_W * mark_scale, HALF_H * mark_scale),
        mark_scale,
        LEAF_STOPS,
    )
    cn, _ = text_ink("枫阅", load_cn_font(300), INK)
    en, _ = text_ink("MAPLEVIEW", load_latin_font(74), AMBER, spacing=20)

    gap_mark, gap_lines = 96, 34
    text_w = max(cn.size[0], en.size[0])
    text_h = cn.size[1] + gap_lines + en.size[1]
    width = mark.size[0] + gap_mark + text_w
    height = max(mark.size[1], text_h)

    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    canvas.alpha_composite(mark, (0, (height - mark.size[1]) // 2))
    tx = mark.size[0] + gap_mark
    ty = (height - text_h) // 2
    canvas.alpha_composite(cn, (tx, ty))
    canvas.alpha_composite(en, (tx, ty + cn.size[1] + gap_lines))
    return pad_canvas(canvas, 48)


def main():
    save(render_mark(1024), "mapleview-mark-1024.png")
    save(render_mark(512), "mapleview-mark-512.png")
    save(render_icon(1024), "mapleview-icon-1024.png")
    save(render_icon(512), "mapleview-icon-512.png")
    save(render_icon(256), "mapleview-icon-256.png")
    save(render_lockup(), "mapleview-logo.png")


if __name__ == "__main__":
    main()
