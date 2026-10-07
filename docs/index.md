---
title: 快如闪电的图片预览
description: 枫阅是一个用 Rust 写的桌面图片查看器：20+ 格式、GPU 渲染、后台多线程解码、相邻图片预取与字节预算缓存，翻页基本零等待。
hide:
  - navigation
  - toc
---

## 上面那个演示不是特效

你刚才看到的每一毫秒都是这台机器上真正跑出来的，不是提前画好的动画。
演示台里放了两种模式，点一下就能对比出差别：

| 模式 | 它是怎么做的 | 你会看到什么 |
| --- | --- | --- |
| ⚡ **枫阅** | 提前准备好相邻的图片，翻回看过的图直接显示 | 第一次约 10~40 ms，再翻回来是**瞬间** |
| 🐌 **其他看图工具** | 每翻一页都从头把整张图重新解一遍 | 每次都等一样久，红色数字一直在跳 |

它跑在浏览器里，但用的是和桌面端同一套办法：提前准备 + 缓存，所以你在浏览器里
感受到的那个差距，就是桌面端翻相册时的差距。

## 主要功能

「快」是它最想被记住的一点，但快不是全部。下面是枫阅现在就能做的事：

<div class="mx-cards">
  <div class="mx-card">
    <span class="mx-card__glyph">🗂️</span>
    <h3>把文件夹当相册翻</h3>
    <p>打开单个文件、整个文件夹、命令行传路径，或者直接把图片拖进窗口都行。同目录的
       兄弟文件自动就位，按文件名自然排序（<code>img2</code> 排在 <code>img10</code> 前面），
       方向键一路翻到底。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🔍</span>
    <h3>缩放像拿放大镜</h3>
    <p>滚轮以光标为锚点缩放，光标下的那个像素不会跑。适应窗口、按宽度、100% 像素级、
       自由倍率随时切换；放大超过 100% 自动切最近邻采样，像素边缘不糊。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">⌨️</span>
    <h3>键盘就能翻完整个相册</h3>
    <p><kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd>、空格、<kbd>Home</kbd> /
       <kbd>End</kbd> 切图，<kbd>F11</kbd> 全屏，<kbd>I</kbd> 看完整 EXIF，
       <kbd>H</kbd> 调出快捷键表。双击在「适应窗口」和「100%」之间来回。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🧾</span>
    <h3>格式不挑食</h3>
    <p>内置 20+ 种格式：PNG / APNG、JPEG、GIF、WebP、BMP、TIFF、TGA、ICO、QOI、
       HDR、EXR、DDS、AVIF……格式判定以文件头为准，把 <code>.RAW</code> 改名成
       <code>.jpg</code> 也照样认得出来。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🖼️</span>
    <h3>巨图不崩</h3>
    <p>超过 64 MP 的图自动降采样显示，而不是拒绝或 OOM；512 MP 以内都能坦然打开。
       竖向拍摄的照片靠 EXIF 方向自动转正，不会再横过来。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🖥️</span>
    <h3>跨平台，还能接进脚本</h3>
    <p>Windows / macOS / Linux 都能跑，渲染走 wgpu。附带无界面的
       <code>mapleview-cli</code>，与 GUI 共用同一条解码路径，<code>info</code> /
       <code>thumb</code> / <code>bench</code> 可以直接接进脚本和 CI。</p>
  </div>
</div>

## 为什么这么快

<div class="mx-cards">
  <div class="mx-card">
    <span class="mx-card__glyph">🧵</span>
    <h3>后台解码，界面不卡</h3>
    <p>图片在后台慢慢解，界面只管显示。翻得再快也不会突然跳回上一张 ——
       已经被你翻过去的请求直接丢掉，不占位置。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">📥</span>
    <h3>前后两张提前备好</h3>
    <p>你还在看这一张的时候，相邻的两张已经在后面解好了。翻相册总是一张张往下走，
       提前准备两张，命中一次就赚回来。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">⚖️</span>
    <h3>缓存按内存大小算</h3>
    <p>不是「最多缓存 100 张」而是「最多占用 1 GiB」。一张 3000×2000 的图是 24 MB，
       一张 100 MP 的扫描件是 400 MB —— 按张数算，迟早会栽在大图上。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🔬</span>
    <h3>要放大才补细节</h3>
    <p>先按屏幕真正需要的大小出图，等你放大到 100% 看像素时，再在后台补上完整分辨率。
       补细节的那一刻，画面不会跳。</p>
  </div>
</div>

## 支持的格式

格式判定看**文件头**而不是扩展名，所以把 `IMG_0001.RAW` 改名成 `photo.jpg`，
枫阅也照样认得出来它到底是什么。

<div class="mx-badges">
  <span class="mx-badge mx-badge--on">PNG</span>
  <span class="mx-badge mx-badge--on">APNG</span>
  <span class="mx-badge mx-badge--on">JPEG</span>
  <span class="mx-badge mx-badge--on">GIF</span>
  <span class="mx-badge mx-badge--on">WebP</span>
  <span class="mx-badge mx-badge--on">BMP</span>
  <span class="mx-badge mx-badge--on">TIFF</span>
  <span class="mx-badge mx-badge--on">TGA</span>
  <span class="mx-badge mx-badge--on">ICO / CUR</span>
  <span class="mx-badge mx-badge--on">QOI</span>
  <span class="mx-badge mx-badge--on">Radiance HDR</span>
  <span class="mx-badge mx-badge--on">OpenEXR</span>
  <span class="mx-badge mx-badge--on">farbfeld</span>
  <span class="mx-badge mx-badge--on">Netpbm</span>
  <span class="mx-badge mx-badge--on">DDS</span>
  <span class="mx-badge mx-badge--on">AVIF</span>
  <span class="mx-badge mx-badge--soft">HEIC / HEIF</span>
  <span class="mx-badge mx-badge--soft">JXL</span>
  <span class="mx-badge mx-badge--soft">SVG</span>
  <span class="mx-badge mx-badge--soft">PSD</span>
  <span class="mx-badge mx-badge--soft">RAW</span>
  <span class="mx-badge mx-badge--soft">PDF</span>
  <span class="mx-badge mx-badge--soft">视频</span>
</div>

实线是内置解码；虚线是**已识别**——枫阅会告诉你它是什么格式、缺哪个解码包，
而不是笼统地报一句「无法打开」。细节见[支持格式](formats.md)。

## 遇到问题，或者想提建议

枫阅是 MIT 协议的开源项目。如果它在你机器上比别的看图工具慢，那大概是个 bug ——
带上图片格式和尺寸反馈一下就好。

<div class="mx-cta" style="margin-top:1.4rem">
  <a class="mx-btn mx-btn--primary" href="features/">看看全部功能</a>
  <a class="mx-btn" href="https://github.com/terry-chao/MapleView">在 GitHub 上反馈</a>
</div>

<p class="mx-credit">演示台里的照片来自 Wikimedia Commons 的「特色图片」，各自遵循原始许可证；
出处与署名见<a href="credits/">图片来源</a>。</p>
