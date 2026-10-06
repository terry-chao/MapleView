# 路线图

## 已完成

<ul class="mx-timeline">
  <li class="is-done">
    <b>M0</b> workspace 骨架、窗口、显示图片、适应窗口
  </li>
  <li class="is-done">
    <b>M1</b> 缩放 / 平移、键盘操作、多线程解码、缓存、EXIF 方向、文件夹导航、预取
  </li>
</ul>

## 计划中

<ul class="mx-timeline">
  <li><b>M2</b> 缩略图条、幻灯片、ICC 色彩管理、磁盘缩略图缓存、按视口分辨率解码的完整两级管线</li>
  <li><b>M3</b> 可选编解码包（HEIC / JXL / SVG / RAW / PDF），运行时 <code>libloading</code> 加载</li>
  <li><b>M4</b> 安装包、文件关联、右键菜单、单实例</li>
  <li><b>M5</b> <code>criterion</code> 基准、解码器 fuzz、格式语料库黄金图回归</li>
</ul>

## 已知限制

这些是**现在**确实做不到的事，写在这里而不是藏起来：

- **帧动画**：GIF / WebP 目前只显示第一帧。
- **色彩管理**：没有 ICC 支持，广色域图片会按 sRGB 直接显示，颜色偏淡。
- **mipmap**：缩放后的纹理没有 mipmap（wgpu 后端不支持 `mipmap_mode`）。
  但预览纹理本身就是按视口尺寸解码的，所以正常浏览不会有锯齿问题。
- **超大图**：超过 512 MP 的图（大幅扫描件、全景）会直接报错，需要瓦片解码，
  属于 M2 之后的工作。64 MP ~ 512 MP 之间会自动降采样，不会 OOM。
- **可选格式**：HEIC / JXL / SVG / RAW / PDF / 视频只能识别、不能解码，等 M3。

## 怎么参与

如果枫阅在你机器上比别的看图工具慢，那是个 bug。开 issue 时请带上：

- 图片格式与像素尺寸（`mapleview-cli info` 的输出最好）
- 复现步骤和 `RUST_LOG=mapleview=debug` 的日志
- 如果是某类文件打不开，尽量提供一张可以公开的最小样本
