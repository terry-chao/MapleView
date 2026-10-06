# 命令行

`mapleview-cli` 是无界面工具，用来验证格式支持和做性能基线。它和 GUI
走的是**完全同一条解码路径**（同一个 `mapleview-core`），所以这里测出来的数字是有意义的。

```powershell
cargo build --release -p mapleview-cli
```

## `info` —— 看格式、尺寸、EXIF、解码耗时

```powershell
mapleview-cli info photo.jpg
mapleview-cli info *.tif --header-only
```

`--header-only` 只读文件头，不解码像素 —— 用来快速筛一遍一个目录里哪些文件是真的坏了。

## `thumb` —— 生成缩略图

```powershell
mapleview-cli thumb photo.jpg --size 256 --out thumb.png
```

内部走的正是应用里「按目标尺寸解码」的那条路径，所以它也是这条路径的可复现基准。

## `bench` —— 解码基准

```powershell
mapleview-cli bench photo.jpg --runs 20
```

建议在 release 下跑，并且输入图放在本地磁盘而不是网络盘上。

## 为什么单独做个 CLI

因为「快」和「格式全」这两件事必须**能测量**。GUI 里的数字会被合成器、
vsync 和窗口大小影响；CLI 里的数字只反映解码器本身。

`crates/core` 与界面完全解耦，命令行工具和 GUI 共用它，这也是核心逻辑能单独写测试的原因。
