# 快速开始

## 要求

- Rust **1.85+**（项目使用 edition 2024）
- Windows / macOS / Linux 桌面环境；渲染走 wgpu，所以 Vulkan / D3D12 / Metal 任一可用即可
- 可选：`libheif`、`libjxl`、`resvg`、`libraw`、`pdfium`、`ffmpeg`（见[支持格式](formats.md)）

## 从源码构建

```powershell
git clone https://github.com/terry-chao/MapleView
cd MapleView

# 只构建桌面端
cargo build --release -p mapleview-app
```

workspace 里有三个二进制，所以从仓库根目录直接 `cargo run` 会因为没有默认包而报错 ——
这是刻意的（见 [Cargo.toml](https://github.com/terry-chao/MapleView/blob/main/Cargo.toml) 里的注释）。

```powershell
# 打开一张图（同时加载同目录的兄弟文件，方便左右键翻页）
.\target\release\mapleview.exe D:\photos\IMG_0001.jpg

# 打开整个文件夹
.\target\release\mapleview.exe D:\photos

# 或者一步到位
cargo run --release -p mapleview-app -- D:\photos\IMG_0001.jpg
```

也可以用图形界面里的 **打开文件**（`Ctrl+O`）或把文件直接拖进窗口。

## 跑测试

测试要对整个 workspace 跑，否则只测到 `mapleview-app`：

```powershell
cargo test --workspace
```

核心解码套件在 `crates/core/tests/decode.rs`，它会对仓库里生成的样例图做端到端断言。

## 日志

日志走 `tracing`，用 `RUST_LOG` 控制：

```powershell
$env:RUST_LOG = 'mapleview=debug,mapleview_core=debug'
.\target\release\mapleview.exe D:\photos
```

| 目标 | 你能看到什么 |
| --- | --- |
| `mapleview_core::decode` | 每张图的格式探测、目标尺寸、解码耗时 |
| `mapleview_core::cache` | 命中 / 未命中、字节预算、淘汰了谁 |
| `mapleview::loader` | 请求版本号、被取消的请求、预取队列 |

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
| `I` | 信息面板（含完整 EXIF） |
| `H` | 快捷键窗口 |
| `F11` / `Esc` | 进入 / 退出全屏 |
| `Ctrl+O` | 打开文件 |

## 下一步

- 想知道它为什么快：[架构](architecture.md)
- 想看实测数字：[性能](performance.md)
- 想接脚本或跑基准：[命令行](cli.md)
