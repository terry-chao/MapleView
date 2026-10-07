# 支持格式

格式判定以**文件头 magic bytes 为准**，扩展名只作兜底。所以改过名的文件也能正确打开 ——
把 `IMG_0001.RAW` 改成 `photo.jpg`，枫阅会告诉你它其实是什么。

## 内置可解码

| 格式 | 说明 |
| --- | --- |
| PNG / APNG | 支持 APNG 容器（当前只显示第一帧） |
| JPEG | 通过 `zune-jpeg`，含 EXIF 方向 |
| GIF | 只显示第一帧 |
| WebP | 有损 / 无损 / 动图容器 |
| BMP / TIFF / TGA | 常规位图 |
| ICO / CUR | 会挑出最合适的那一档尺寸 |
| QOI | Quite OK Image |
| Radiance HDR / OpenEXR | 浮点高动态范围 |
| farbfeld / Netpbm | 简单无损格式 |
| DDS | 含压缩纹理变体 |
| AVIF | 通过 `ravif` / `rav1e` 解码路径 |

## 能识别，但还打不开

下面这些格式枫阅能通过文件头**认出来**，也会明确告诉你它是什么、缺哪个解码器，
而不是笼统地报「无法打开」—— 只是还不能真的把像素解出来。

| 格式 | 需要的解码器 |
| --- | --- |
| HEIC / HEIF | `libheif` |
| JPEG XL | `libjxl` |
| SVG | `resvg` |
| PSD | 内置 PSD 解析 |
| RAW | `libraw` |
| PDF | `pdfium` |
| 视频帧 | `ffmpeg` |

## 大图策略

| 像素数 | 行为 |
| --- | --- |
| ≤ 64 MP | 正常解码（覆盖市面上所有消费级相机） |
| 64 MP ~ 512 MP | 自动降采样到 64 MP 再显示，不拒绝、不 OOM |
| > 512 MP | 报错并说明原因 |

64 MP 这个上限对应最坏情况 256 MiB 的 RGBA 分配（`DEFAULT_MAX_DECODE_PIXELS`）。
