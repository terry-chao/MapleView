#!/usr/bin/env python3
"""抓取官网演示图集：来自 Wikimedia Commons 的自由版权照片。

官网首页的「闪电预览」演示台需要一组**值得看**的照片 —— 早先的图集是
脚本程序化生成的抽象渐变，够用但不好看，也不像真实照片。这个脚本是
`docs/assets/demo/` 里那些文件的来源：从 Wikimedia Commons 拉取
「特色图片 / 优质图片」，只保留允许再利用的许可证，缩放到演示需要的
尺寸，并写出浏览器读的 manifest 和一份署名清单。

它需要联网，所以**不参与**站点构建：生成的图片会提交进仓库，CI 只跑
`mkdocs build`。只有想换图时才重新跑一次：

    python tools/fetch_demo_photos.py --probe           # 只打印候选，不下载
    python tools/fetch_demo_photos.py                   # 下载 + 处理 + 写清单
    python tools/fetch_demo_photos.py --only=01-viaduct # 只重下某一张

换图时改下面的 SELECTION 常量；每张图的署名会自动写进 docs/credits.md。
品牌资源（favicon / 图标 / 社交卡片）仍然是程序化生成的，见
`tools/gen_site_assets.py`。
"""

from __future__ import annotations

import argparse
import html
import io
import json
import os
import re
import sys
import urllib.parse
import urllib.request

from PIL import Image, ImageOps

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEMO_DIR = os.path.join(ROOT, "docs", "assets", "demo")
JS_DIR = os.path.join(ROOT, "docs", "assets", "javascripts")
DOCS_DIR = os.path.join(ROOT, "docs")

API = "https://commons.wikimedia.org/w/api.php"
UA = "MapleView-site-assets/1.0 (https://github.com/terry-chao/MapleView)"

MAX_EDGE = 2560          # 长边。演示台按视口解码，2560 足够放大到 100% 看清楚，
                         # 同时把整个图集压在 6 MB 上下，别让"快"的官网自己先慢下来。
THUMB_WIDTH = 3000       # 让服务端直接给出够大的缩略图，别拉原始 100 MB 文件
JPEG_QUALITY = 80        # 4:2:0 采样；真实照片在这个档位肉眼基本看不出损失
WEBP_QUALITY = 80

# 认不出许可证的文件一律跳过，只留下明确可再利用的。
OK_LICENSE = re.compile(r"cc0|public domain|pd-|cc by", re.I)

# --------------------------------------------------------------------------- #
# 精选图集：换图时改这里。slug 决定文件名，title 是界面上显示的中文名。
#
# 全部取自 Wikimedia Commons 的「特色图片」——同行评审挑出来的作品，
# 拿来当看图器的素材再合适不过。许可证都在允许再利用的范围内，
# 具体署名由脚本写进 docs/credits.md。
# --------------------------------------------------------------------------- #
SELECTION: list[dict] = [
    {
        "slug": "01-viaduct",
        "title": "石拱高架桥",
        "format": "JPEG",
        "file": "File:2015 Ribblehead Viaduct 1.jpg",
    },
    {
        "slug": "02-lake",
        "title": "湖光暮色",
        "format": "JPEG",
        "file": "File:Evening on Kathleen Lake.jpg",
    },
    {
        "slug": "03-glacier",
        "title": "冰川蓝湖",
        "format": "JPEG",
        "file": "File:Blue Lake in Mount Cook National Park.jpg",
    },
    {
        "slug": "04-sunrise",
        "title": "高原日出",
        "format": "JPEG",
        "file": "File:Amanecer en el lago Titicaca, Puno, Perú, 2015-08-01, DD 01.JPG",
    },
    {
        "slug": "05-aurora",
        "title": "极光与流星",
        "format": "JPEG",
        "file": "File:Aurora and perseids.jpg",
    },
    {
        "slug": "06-comet",
        "title": "彗星夜空",
        "format": "JPEG",
        "file": "File:Comet Tsuchinshan–ATLAS, in the night sky over Tuntorp 3.jpg",
    },
    {
        "slug": "07-kyoto",
        "title": "京都夜灯",
        "format": "JPEG",
        "file": "File:Kimono Forest at night, Arashiyama Station, Arashiyama, Kyoto, Japan.jpg",
    },
    {
        "slug": "08-station",
        "title": "穹顶车站",
        "format": "JPEG",
        "file": "File:Antwerp Central Station full size.jpg",
    },
    {
        "slug": "09-dew",
        "title": "草叶露珠",
        "format": "JPEG",
        "file": "File:Dew on grass Luc Viatour.jpg",
    },
    {
        "slug": "10-mist",
        "title": "晨雾湖面",
        "format": "JPEG",
        "file": "File:Oever van het meer in de mist. Locatie, Langweerderwielen (Langwarder Wielen).jpg",
    },
    {
        "slug": "11-nebula",
        "title": "船底座星云",
        "format": "JPEG",
        "file": "File:Carina Nebula.jpg",
    },
    {
        "slug": "12-pavilion",
        "title": "宫阙倒影",
        "format": "WebP",
        "file": "File:Water reflection of Hyangwonjeong Pavilion at Gyeongbokgung Palace in Seoul.jpg",
    },
]

LICENSE_URL = {
    "cc0": "https://creativecommons.org/publicdomain/zero/1.0/",
    "cc by 4.0": "https://creativecommons.org/licenses/by/4.0/",
    "cc by 3.0": "https://creativecommons.org/licenses/by/3.0/",
    "cc by 2.0": "https://creativecommons.org/licenses/by/2.0/",
    "cc by 2.5": "https://creativecommons.org/licenses/by/2.5/",
    "cc by-sa 4.0": "https://creativecommons.org/licenses/by-sa/4.0/",
    "cc by-sa 3.0": "https://creativecommons.org/licenses/by-sa/3.0/",
    "cc by-sa 2.5": "https://creativecommons.org/licenses/by-sa/2.5/",
}

EXT = {"JPEG": "jpg", "PNG": "png", "WebP": "webp"}


# --------------------------------------------------------------------------- #
# Commons 接口
# --------------------------------------------------------------------------- #
def api(params: dict) -> dict:
    params = dict(params)
    params.setdefault("format", "json")
    params.setdefault("formatversion", "2")
    url = API + "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)


def plain(value: str | None) -> str:
    """把 Commons 返回的 HTML 片段压成一行纯文本。"""
    if not value:
        return ""
    text = re.sub(r"<[^>]+>", " ", value)
    return re.sub(r"\s+", " ", html.unescape(text)).strip()


def candidates(category: str, limit: int = 100) -> list[dict]:
    data = api(
        {
            "action": "query",
            "generator": "categorymembers",
            "gcmtitle": category,
            "gcmtype": "file",
            "gcmlimit": str(limit),
            "prop": "imageinfo",
            "iiprop": "url|size|mime|extmetadata",
            "iiurlwidth": str(THUMB_WIDTH),
        }
    )
    out: list[dict] = []
    for page in (data.get("query", {}) or {}).get("pages", []) or []:
        info = (page.get("imageinfo") or [{}])[0]
        meta = info.get("extmetadata", {}) or {}
        lic = plain((meta.get("LicenseShortName") or {}).get("value"))
        out.append(
            {
                "title": page.get("title", ""),
                "width": info.get("thumbwidth") or info.get("width"),
                "height": info.get("thumbheight") or info.get("height"),
                "license": lic,
                "ok": bool(info.get("thumburl")) and bool(OK_LICENSE.search(lic)),
            }
        )
    return out


def lookup(title: str) -> dict:
    """取单个文件的缩略图地址与署名信息。"""
    data = api(
        {
            "action": "query",
            "titles": title,
            "prop": "imageinfo",
            "iiprop": "url|size|mime|extmetadata",
            "iiurlwidth": str(THUMB_WIDTH),
        }
    )
    pages = (data.get("query", {}) or {}).get("pages", []) or []
    if not pages or "missing" in pages[0]:
        raise SystemExit(f"Commons 上找不到：{title}")
    info = (pages[0].get("imageinfo") or [{}])[0]
    meta = info.get("extmetadata", {}) or {}
    lic = plain((meta.get("LicenseShortName") or {}).get("value"))
    if not OK_LICENSE.search(lic):
        raise SystemExit(f"许可证不可再利用（{lic}）：{title}")
    if not info.get("thumburl"):
        raise SystemExit(f"没有可用的缩略图：{title}")
    return {
        "thumb": info["thumburl"],
        "license": lic,
        "artist": plain((meta.get("Artist") or {}).get("value")) or "未知",
        "page": info.get("descriptionurl", ""),
    }


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=180) as resp:
        return resp.read()


def process(raw: bytes, fmt: str, max_edge: int = MAX_EDGE) -> tuple[bytes, tuple[int, int]]:
    """统一成入库格式，长边限制在 max_edge 以内。"""
    with Image.open(io.BytesIO(raw)) as im:
        im = ImageOps.exif_transpose(im).convert("RGB")
        if max(im.size) > max_edge:
            k = max_edge / max(im.size)
            im = im.resize((round(im.width * k), round(im.height * k)), Image.LANCZOS)
        buf = io.BytesIO()
        if fmt == "JPEG":
            im.save(buf, "JPEG", quality=JPEG_QUALITY, optimize=True, progressive=True)
        elif fmt == "WebP":
            im.save(buf, "WebP", quality=WEBP_QUALITY, method=6)
        elif fmt == "PNG":
            im.save(buf, "PNG", optimize=True, compress_level=9)
        else:
            raise SystemExit(f"不支持的输出格式：{fmt}")
        return buf.getvalue(), im.size


def fmt_bytes(n: int) -> str:
    if n >= 1024 * 1024:
        return f"{n / 1024 / 1024:.1f} MB"
    return f"{n / 1024:.0f} KB"


# --------------------------------------------------------------------------- #
# 命令
# --------------------------------------------------------------------------- #
def probe(categories: list[str], limit: int, out: str | None) -> None:
    lines: list[str] = []
    for category in categories:
        rows = candidates(category, limit)
        usable = [r for r in rows if r["ok"]]
        lines.append(f"# {category}: {len(rows)} 个文件，其中 {len(usable)} 个许可证可用\n")
        for r in sorted(usable, key=lambda r: (r["width"] or 0) * (r["height"] or 0), reverse=True):
            lines.append(f"{r['width']:>5}×{r['height']:<5}  {r['license']:<18}  {r['title']}")
        lines.append("")
    report = "\n".join(lines)
    if out:
        with open(out, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(report + "\n")
        print(f"wrote {out}")
    else:
        print(report)


def build(only: str | None) -> int:
    os.makedirs(DEMO_DIR, exist_ok=True)
    os.makedirs(JS_DIR, exist_ok=True)

    # demo/ 目录只放图集，先把上一次的产出清干净，换图后不留孤儿文件。
    for name in os.listdir(DEMO_DIR):
        if os.path.splitext(name)[1].lower() in (".jpg", ".jpeg", ".png", ".webp"):
            os.remove(os.path.join(DEMO_DIR, name))

    manifest: list[dict] = []
    credits: list[dict] = []
    total = 0
    for entry in SELECTION:
        if only and entry["slug"] != only:
            continue
        print(f"  {entry['slug']:<14} {entry['format']:<5} {entry['file']}", flush=True)
        info = lookup(entry["file"])
        data, size = process(fetch(info["thumb"]), entry["format"])
        name = f"{entry['slug']}.{EXT[entry['format']]}"
        with open(os.path.join(DEMO_DIR, name), "wb") as fh:
            fh.write(data)
        n = len(data)
        total += n
        manifest.append(
            {
                "file": name,
                "title": entry["title"],
                "format": entry["format"],
                "w": size[0],
                "h": size[1],
                "bytes": n,
            }
        )
        credits.append({**entry, "name": name, "w": size[0], "h": size[1], **info})
        print(f"  {'':14} {'':5} -> {name} {size[0]}×{size[1]} {fmt_bytes(n)}", flush=True)

    if only:
        print("只重下了单张图，manifest / credits 未改动")
        return 0

    manifest_js = (
        "// Generated by tools/fetch_demo_photos.py -- do not edit by hand.\n"
        "window.MV_CORPUS = "
        + json.dumps(manifest, ensure_ascii=False, indent=2)
        + ";\n"
    )
    with open(os.path.join(JS_DIR, "demo-corpus.js"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write(manifest_js)
    write_credits(credits)
    print(f"\ncorpus total: {fmt_bytes(total)} across {len(manifest)} images")
    return 0


def write_credits(credits: list[dict]) -> None:
    lines = [
        "# 演示图片版权",
        "",
        "官网首页演示台里的照片全部来自 [Wikimedia Commons](https://commons.wikimedia.org/)",
        "的「特色图片」（Featured pictures）——由社区同行评审挑出的作品，",
        "许可证允许再利用。下面按演示里的顺序列出每一张的出处与署名。",
        "",
        "这些图片**不属于** MIT 许可的代码部分，各自遵循其原始许可证。",
        "如果你要复用它们，请按下方许可证要求署名。",
        "",
    ]
    for c in credits:
        lic_key = c["license"].strip().lower()
        lic_url = LICENSE_URL.get(lic_key)
        lic = f"[{c['license']}]({lic_url})" if lic_url else c["license"]
        lines += [
            f"## {c['title']}（`{c['name']}`）",
            "",
            f"- 来源：[{c['file']}]({c['page']})",
            f"- 作者：{c['artist']}",
            f"- 许可证：{lic}",
            f"- 尺寸：{c['w']} × {c['h']}",
            "",
        ]
    lines += [
        "## 徽标",
        "",
        "应用图标与品牌图片由本仓库自行绘制，属于项目本身的资产。",
        "",
        "图集由 `tools/fetch_demo_photos.py` 生成，换图时改脚本顶部的 `SELECTION`。",
        "",
    ]
    with open(os.path.join(DOCS_DIR, "credits.md"), "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(lines))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--probe", action="store_true", help="只打印候选，不下载")
    ap.add_argument("--category", nargs="+", default=["Category:Featured pictures of landscapes"], help="要拉取的 Commons 分类")
    ap.add_argument("--out", help="把候选清单写到文件（UTF-8），避免控制台编码问题")
    ap.add_argument("--limit", type=int, default=100, help="每个分类最多取多少个候选")
    ap.add_argument("--only", help="只重新处理某一个 slug")
    args = ap.parse_args()

    if args.probe:
        probe(args.category, args.limit, args.out)
        return 0
    return build(args.only)


if __name__ == "__main__":
    sys.exit(main())
