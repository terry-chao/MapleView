---
title: MapleView — 快如闪电的图片预览
description: MapleView 是一个用 Rust 写的桌面图片查看器：20+ 格式、GPU 渲染、后台多线程解码、相邻图片预取与字节预算缓存，翻页基本零等待。
hide:
  - navigation
  - toc
---

## 上面那个演示不是特效

你刚才看到的每一毫秒都是这台机器上跑出来的：图片由 Web Worker 用
`createImageBitmap` 真正解码，缓存按**字节**计费，翻页时旧请求会被版本号取消。
页面把这个过程做成了可比较的两条管线 —— 点一下就能感受到差别：

| 管线 | 行为 | 你该看到什么 |
| --- | --- | --- |
| ⚡ **闪电管线** | 预取邻居 + 字节预算缓存 + 按视口分辨率解码 | 第一次约 10~40 ms，之后翻回同一张是**零解码** |
| 🐌 **朴素管线** | 无预取、无缓存，每次都把原图整张解出来 | 每次都是同样的等待，红色数字一直在跳 |

这不是把桌面端搬进浏览器，而是**同一套策略**的浏览器实现。桌面端的瓶颈在解码器
而不是界面逻辑，所以这些取舍在两端是通用的。

## 快，来自四个决定

<div class="mx-cards">
  <div class="mx-card">
    <span class="mx-card__glyph">🧵</span>
    <h3>解码离开 UI 线程</h3>
    <p>解码跑在线程池里，界面线程只负责画。切图时用一个单调递增的版本号标记请求，
       旧的那张图即使解完了也不会上屏 —— 不会出现「翻得快就跳到上一张」。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">📥</span>
    <h3>邻居提前搬家</h3>
    <p>显示第 N 张的同时，第 N±1、N±2 张已经开始解码。人在相册里的移动是局部的，
       猜错两次的代价远小于猜对一次的收益。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">⚖️</span>
    <h3>缓存按字节计费</h3>
    <p>不是「缓存 100 张」而是「缓存 1 GiB」。一张 3000×2000 的 RGBA 是 24 MB，
       一张 100 MP 扫描件是 400 MB —— 按张数计费的缓存迟早会在其中一边翻车。</p>
  </div>
  <div class="mx-card">
    <span class="mx-card__glyph">🔬</span>
    <h3>按视口解码</h3>
    <p>先把大图解成屏幕上真正需要的那点分辨率，放大到 100% 以上时才在后台换成
       全分辨率纹理。几何按源图像素计算，所以换纹理的那一刻画面不会跳。</p>
  </div>
</div>

## 解码流水线

每一张图都走同一条路，每一步都能单独测试：

<div class="mx-flow">
  <span>读文件</span><i>→</i>
  <span>magic bytes 探测格式</span><i>→</i>
  <span>读文件头拿尺寸</span><i>→</i>
  <span>解码</span><i>→</i>
  <span>EXIF 方向校正</span><i>→</i>
  <span>按目标尺寸缩放</span><i>→</i>
  <span>RGBA 上屏</span>
</div>

格式判定看**文件头**而不是扩展名，所以把 `IMG_0001.RAW` 改名成 `photo.jpg`
也照样能正确打开 —— 只会告诉你它其实是什么。

## 实测数据

Windows / x86_64，`cargo build --release`，6 MP JPEG（3000×2000）：

| 场景 | 结果 |
| --- | --- |
| 全分辨率解码 | **14.6 ms** 平均，约 410 MP/s |
| 解码到 256 px 缩略图 | 约 24 ms |
| 启动到显示第一张图 | 约 0.6 s（含按视口尺寸解码 3000×2000 → 2048×1365） |
| 二进制体积 | GUI 15.2 MB，CLI 3.7 MB |

Debug 构建下同样一张图约 18 ms —— 说明瓶颈在解码本身，而不在应用逻辑上。
完整方法和复现命令见[性能](performance.md)。

## 能打开的格式

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

实线是内置解码；虚线是**已识别**、装上可选编解码包即可打开 —— 会给出明确提示，
而不是笼统的「无法打开」。细节见[支持格式](formats.md)。

## 两分钟跑起来

需要 Rust 1.85+（edition 2024）。

```powershell
git clone https://github.com/terry-chao/MapleView
cd MapleView
cargo build --release

# 打开一张图（会顺带加载同目录的兄弟文件，方便左右翻页）
.\target\release\mapleview.exe D:\photos\IMG_0001.jpg

# 或者整个文件夹
.\target\release\mapleview.exe D:\photos
```

也可以直接 `cargo run --release -p mapleview-app -- D:\photos`。
更多细节见[快速开始](start.md)。

## 路线图

<ul class="mx-timeline">
  <li class="is-done"><b>M0</b> workspace 骨架、开窗、显示图片、适应窗口</li>
  <li class="is-done"><b>M1</b> 缩放/平移、键盘操作、多线程解码、缓存、EXIF 方向、文件夹导航、预取</li>
  <li><b>M2</b> 缩略图条、幻灯片、ICC 色彩管理、磁盘缩略图缓存、完整的两级解码管线</li>
  <li><b>M3</b> 可选编解码包（HEIC / JXL / SVG / RAW / PDF），运行时 <code>libloading</code> 加载</li>
  <li><b>M4</b> 安装包、文件关联、右键菜单、单实例</li>
  <li><b>M5</b> criterion 基准、解码器 fuzz、格式语料库黄金图回归</li>
</ul>

现在处于 **M1 完成**。诚实的已知限制都写在[路线图](roadmap.md)里 ——
比如帧动画目前只显示第一帧，也没有 ICC 色彩管理。

## 想要更快，或者想吐槽

MapleView 是 MIT 协议的开源项目，issue 和 PR 都欢迎。
如果它在你机器上比别的看图工具慢，那是个 bug，请带上格式和尺寸开个 issue。

<div class="mx-cta" style="margin-top:1.4rem">
  <a class="mx-btn mx-btn--primary" href="https://github.com/terry-chao/MapleView">在 GitHub 上查看</a>
  <a class="mx-btn" href="start/">从源码开始</a>
</div>
