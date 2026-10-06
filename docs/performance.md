# 性能

## 测试环境

- Windows / x86_64
- `cargo build --release`（`lto = "thin"`、`codegen-units = 1`、`panic = "abort"`）
- 输入：6 MP JPEG（3000×2000）

## 结果

| 场景 | 结果 |
| --- | --- |
| 全分辨率解码 | **14.6 ms** 平均，约 410 MP/s |
| 解码到 256 px 缩略图 | 约 24 ms |
| 启动到显示第一张图 | 约 0.6 s（含按视口尺寸解码 3000×2000 → 2048×1365） |
| 二进制体积 | GUI 15.2 MB，CLI 3.7 MB |

Debug 构建下同样一张图约 18 ms —— 只比 release 慢一点点。这说明瓶颈在解码器本身，
而不是应用逻辑；也说明调优应该往解码器里找，而不是去优化事件循环。

## 复现

```powershell
cargo build --release

# 单张图解码 20 次
.\target\release\mapleview-cli.exe bench D:\photos\IMG_0001.jpg --runs 20

# 只看文件头，验证格式识别
.\target\release\mapleview-cli.exe info D:\photos\*.jpg --header-only

# 缩略图路径
.\target\release\mapleview-cli.exe thumb D:\photos\IMG_0001.jpg --size 256 --out t.png
```

## 瓶颈在哪里

整条流水线是：

<div class="mx-flow">
  <span>读文件</span><i>→</i>
  <span>探测格式</span><i>→</i>
  <span>读文件头</span><i>→</i>
  <span>解码</span><i>→</i>
  <span>EXIF 旋转</span><i>→</i>
  <span>缩放</span><i>→</i>
  <span>RGBA</span>
</div>

除了「解码」，其余每一步都在微秒到亚毫秒量级。所以真正影响体感的是两件事：

1. **解码在哪个线程上发生** —— 在 UI 线程上就是卡顿
2. **解码出多少像素** —— 3000×2000 全解是 6 MP 的工作量，按视口只需要 2048×1365

枫阅的答案分别是「线程池 + 版本号取消」和「`DecodeHint` 指定目标尺寸」。

## 体积

| 二进制 | 体积 |
| --- | --- |
| `mapleview.exe`（GUI） | 15.2 MB |
| `mapleview-cli.exe` | 3.7 MB |

大部分体积来自 GPU 后端（wgpu + naga）和各种解码器。release profile 里
`strip = "symbols"` 已经打开了。
