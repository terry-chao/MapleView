# MapleView

一个用 Rust 写的图片查看器，目标是**快、顺手、格式尽可能全**。

目前处于 **M1 完成** 的状态：能打开单个文件或整个文件夹，GPU 渲染，滚轮以光标为中心缩放，拖拽平移，键盘切图，后台多线程解码 + 预取 + 字节预算缓存。

## 现在能用什么

**查看**

- 打开文件（`Ctrl+O`）、打开文件夹、命令行传路径、拖拽进窗口
- 文件夹内前后翻页，按文件名自然排序（`img2` 排在 `img10` 前面）
- 适应窗口 / 按宽度自动适配 / 100% 像素级显示 / 自由缩放（上限 6400%）
- 滚轮以光标为锚点缩放；放大超过 100% 时自动切换最近邻采样，像素不糊
- 拖拽平移，并且不会把图片拖出屏幕
- 全屏（`F11`），双击在「适应窗口」和「100%」之间切换
- EXIF 方向自动校正，信息面板显示完整 EXIF

**性能相关**

- 解码在后台线程池，导航时旧请求会被取消（版本号机制），不会白算
- 相邻图片自动预取进缓存，翻页基本零等待
- 缓存按**字节**计费而不是按张数，默认上限 1 GiB（`CACHE_BUDGET_BYTES`）
- 按视口分辨率解码预览图，放大到 100% 时再后台换成全分辨率纹理，几何不变所以不会跳
- 超过 64 MP 的图自动降采样显示而不是拒绝或 OOM；超过 512 MP 才报错

**支持的格式**

内置可解码：PNG/APNG、JPEG、GIF、WebP、BMP、TIFF、TGA、ICO/CUR、QOI、Radiance HDR、OpenEXR、farbfeld、Netpbm、DDS、AVIF。

已识别但需要可选编解码包（会给出明确提示，而不是笼统报错）：HEIC/HEIF（libheif）、JPEG XL（libjxl）、SVG（resvg）、PSD、RAW（libraw）、PDF（pdfium）、视频（ffmpeg）。

格式判定以**文件头 magic bytes 为准**，扩展名只作兜底，所以改过名的文件也能正确打开。

## 构建与运行

```powershell
cargo build --release

# 从仓库根目录直接运行（workspace 里有多个二进制，所以需要指定包）
cargo run --release -p mapleview-app -- D:\photos\IMG_0001.jpg

# 打开一张图（同时会加载同目录的兄弟文件，方便左右键翻页）
.\target\release\mapleview.exe D:\photos\IMG_0001.jpg

# 打开整个文件夹
.\target\release\mapleview.exe D:\photos
```

测试要对整个 workspace 跑，否则只测到 `mapleview-app`：

```powershell
cargo test --workspace
```

日志用 `RUST_LOG` 控制：

```powershell
$env:RUST_LOG = 'mapleview=debug,mapleview_core=debug'
```

## 命令行工具

`mapleview-cli` 是无界面的，用来验证格式支持和做性能基线：

```powershell
# 看格式、尺寸、EXIF、解码耗时（--header-only 只读文件头，不解码）
mapleview-cli info photo.jpg
mapleview-cli info *.tif --header-only

# 生成缩略图
mapleview-cli thumb photo.jpg --size 256 --out thumb.png

# 解码基准测试
mapleview-cli bench photo.jpg --runs 20
```

## 快捷键

| 按键 | 作用 |
| --- | --- |
| `←` `→` / `↑` `↓` | 上一张 / 下一张 |
| `空格` / `PageDown` | 下一张 |
| `Home` / `End` | 第一张 / 最后一张 |
| 滚轮 | 以光标为中心缩放 |
| 拖拽 | 平移 |
| 双击 | 适应窗口 ↔ 100% |
| `+` / `-` | 放大 / 缩小 |
| `F` 或 `0` | 适应窗口 |
| `1` | 100%（一个图像像素对一个物理像素） |
| `I` | 信息面板 |
| `H` | 快捷键窗口 |
| `F11` / `Esc` | 进入 / 退出全屏 |
| `Ctrl+O` | 打开文件 |

## 代码结构

```
crates/
  core/    mapleview-core   不含 GUI 的全部核心能力
  app/     mapleview-app    egui + wgpu 桌面应用
  cli/     mapleview-cli    无界面工具
```

`core` 与界面完全解耦，所以解码、缓存、导航这些真正值钱的部分可以单独测试，命令行工具和 GUI 走的是同一条解码路径。

解码流水线：

```
读文件 → 探测格式 → 读文件头判断尺寸 → 解码 → EXIF 旋转 → 按目标尺寸缩放 → RGBA
```

关键设计：

- `DecodeHint`（`crates/core/src/decode.rs`）让调用方指定目标尺寸，大图不必先以全分辨率展开
- `ImageCache`（`crates/core/src/cache.rs`）的缓存键是 `(路径, 目标尺寸)`，所以同一张图的预览版和全分辨率版可以共存
- `Loader`（`crates/app/src/loader.rs`）用原子版本号做取消，显示请求和解码线程分离
- `ViewState`（`crates/app/src/view.rs`）的几何全部以**源图像素**为单位，这样预览纹理和全分辨率纹理来回切换时画面不会跳

## 实测数据

Windows / x86_64，release 构建（`cargo build --release`）：

| 场景 | 结果 |
| --- | --- |
| 6 MP JPEG 全分辨率解码 | 14.6 ms 平均，约 410 MP/s |
| 6 MP JPEG 解码到 256px 缩略图 | 约 24 ms |
| 启动到显示第一张图 | 约 0.6 s（含按视口尺寸解码 3000×2000 → 2048×1365） |
| 二进制体积 | GUI 15.2 MB，CLI 3.7 MB |

Debug 构建下同样一张图约 18 ms，说明瓶颈在解码本身而不是应用逻辑。

## 路线图

- [x] **M0** workspace 骨架、窗口、显示图片、适应窗口
- [x] **M1** 缩放/平移、键盘操作、多线程解码、缓存、EXIF 方向、文件夹导航、预取
- [ ] **M2** 缩略图条、幻灯片、ICC 色彩管理、磁盘缩略图缓存、按视口分辨率解码的完整两级管线
- [ ] **M3** 可选编解码包（HEIC / JXL / SVG / RAW / PDF），运行时 `libloading` 加载
- [ ] **M4** 安装包、文件关联、右键菜单、单实例
- [ ] **M5** `criterion` 基准、解码器 fuzz、格式语料库黄金图回归

## 已知限制

- 帧动画（GIF/WebP）目前只显示第一帧。
- 没有 ICC 色彩管理，广色域图片会按 sRGB 直接显示，颜色偏淡。
- 缩放后的纹理没有 mipmap（wgpu 后端不支持 `mipmap_mode`），但预览纹理本身就是按视口尺寸解码的，所以正常浏览不会有锯齿问题。
- 超过 512 MP 的图（大幅扫描件、全景）会直接报错，需要瓦片解码，属于 M2 之后的工作。
