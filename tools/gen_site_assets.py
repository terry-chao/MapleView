"""生成官网的品牌资源：favicon、应用图标、社交分享卡片。

这里只做**程序化、可复现**的品牌图，不联网、不依赖第三方素材：

    python tools/gen_site_assets.py

它写出：

    docs/assets/brand/icon-*.png     favicon 与各尺寸站点图标
    docs/assets/brand/favicon.ico
    docs/assets/brand/social-card.jpg

首页演示台里的照片是另一条线，来自 Wikimedia Commons，见
`tools/fetch_demo_photos.py`。
"""

from __future__ import annotations

import os
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG_DIR = os.path.join(ROOT, "docs", "assets", "brand")
# 桌面端随包发布的图标；站点资源都从它采样，避免两边长得不一样。
ICON_PNG = os.path.join(ROOT, "assets", "logo", "mapleview-icon-512.png")


def brand_icon(size: int) -> Image.Image:
    """The app icon, downsampled from the bitmap the desktop build ships.

    Keeping one source of truth means the favicon, the touch icon and the social
    card can never drift away from the .exe icon again.
    """
    with Image.open(ICON_PNG) as source:
        return source.convert("RGBA").resize((size, size), Image.LANCZOS)


def _font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    """A bold face that can actually draw 枫阅.

    The card title is Chinese, and Segoe UI / Arial ship no CJK glyphs -- Pillow
    would quietly draw .notdef tofu instead. So a CJK-capable bold is tried
    first, and the Latin faces are only a last resort.
    """
    candidates = (
        (r"C:\Windows\Fonts", "msyhbd.ttc"),      # 微软雅黑 Bold
        (r"C:\Windows\Fonts", "simhei.ttf"),      # 黑体
        (r"C:\Windows\Fonts", "Dengb.ttf"),       # 等线 Bold
        ("/usr/share/fonts/opentype/noto", "NotoSansCJK-Bold.ttc"),
        ("/usr/share/fonts/truetype/noto", "NotoSansCJKsc-Bold.otf"),
        (r"C:\Windows\Fonts", "segoeuib.ttf"),
        (r"C:\Windows\Fonts", "arialbd.ttf"),
        ("/usr/share/fonts/truetype/dejavu", "DejaVuSans-Bold.ttf"),
    )
    for root, name in candidates:
        candidate = os.path.join(root, name)
        if os.path.exists(candidate):
            try:
                return ImageFont.truetype(candidate, size)
            except OSError:
                pass
    return ImageFont.load_default(size)


def _card_backdrop(w: int, h: int) -> Image.Image:
    """A soft, deterministic backdrop: dark slate with two warm glows.

    Drawn small and blown up so the gradients stay smooth without a noise
    library -- the card only needs to look calm behind the logo.
    """
    sw, sh = w // 8, h // 8
    base = Image.new("RGB", (sw, sh), (12, 14, 22))
    d = ImageDraw.Draw(base, "RGBA")
    for cx, cy, r, col in (
        (0.78, 0.12, 0.62, (247, 148, 51, 150)),   # 枫叶橙
        (0.10, 0.95, 0.55, (32, 140, 130, 120)),   # 冷绿
        (0.55, 0.55, 0.80, (52, 40, 90, 110)),     # 紫
    ):
        x, y, rr = cx * sw, cy * sh, r * max(sw, sh)
        d.ellipse([x - rr, y - rr, x + rr, y + rr], fill=col)
    return base.resize((w, h), Image.BICUBIC).filter(ImageFilter.GaussianBlur(3))


def social_card(path: str) -> None:
    w, h = 1200, 630
    card = _card_backdrop(w, h)
    d = ImageDraw.Draw(card)
    icon = brand_icon(160)
    card.paste(icon, (72, 72), icon)
    d.text((78, 292), "枫阅", font=_font(88), fill=(255, 255, 255))
    d.text((82, 402), "快如闪电的图片预览  ·  Rust + wgpu", font=_font(36), fill=(247, 178, 75))
    # 1200×630 的社交卡片用 JPEG：PNG 会有半 MB 以上，没必要。
    card.save(path, "JPEG", quality=88, optimize=True, progressive=True)


def main() -> int:
    os.makedirs(IMG_DIR, exist_ok=True)
    for size in (32, 48, 180, 512):
        brand_icon(size).save(os.path.join(IMG_DIR, f"icon-{size}.png"))
    brand_icon(64).save(os.path.join(IMG_DIR, "favicon.ico"), sizes=[(16, 16), (32, 32), (48, 48), (64, 64)])
    social_card(os.path.join(IMG_DIR, "social-card.jpg"))
    print("wrote brand icons + social card")
    return 0


if __name__ == "__main__":
    sys.exit(main())
