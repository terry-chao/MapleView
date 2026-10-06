# 架构

## 三个 crate

```
crates/
  core/    mapleview-core   不含 GUI 的全部核心能力
  app/     mapleview-app    egui + wgpu 桌面应用
  cli/     mapleview-cli    无界面工具
```

`core` 与界面完全解耦，所以解码、缓存、导航这些真正值钱的部分可以单独测试，
命令行工具和 GUI 走的是同一条解码路径。

## 解码流水线

<div class="mx-flow">
  <span>读文件</span><i>→</i>
  <span>探测格式</span><i>→</i>
  <span>读文件头判断尺寸</span><i>→</i>
  <span>解码</span><i>→</i>
  <span>EXIF 旋转</span><i>→</i>
  <span>按目标尺寸缩放</span><i>→</i>
  <span>RGBA</span>
</div>

## 四个关键设计

### `DecodeHint` —— 让调用方指定目标尺寸

`crates/core/src/decode.rs` 里的 `DecodeHint` 允许调用方说「我只要 2048 宽的」。
大图因此不必先以全分辨率展开再缩小 —— 那是两倍内存和一大截时间。

### `ImageCache` —— 缓存键是 `(路径, 目标尺寸)`

`crates/core/src/cache.rs`。同一张图的**预览版和全分辨率版可以共存**，
这正是「先预览、后补全分辨率且画面不跳」的基础。缓存按字节计费，超预算按 LRU 淘汰。

### `Loader` —— 用原子版本号做取消

`crates/app/src/loader.rs`。每次导航把版本号加一，解码线程完成时比对版本号，
过期结果直接丢弃。显示请求和解码线程因此完全解耦：界面从不等待。

### `ViewState` —— 几何全部以源图像素为单位

`crates/app/src/view.rs`。视口变换只跟源图像素有关，跟当前挂的是哪张纹理无关。
所以预览纹理和全分辨率纹理来回切换时，画面**不会跳**。

## 为什么这么分

界面会重写，GPU 后端会换，解码器会增删。但「一张 6 MP 的图该怎么最快变成屏幕上的像素」
这个问题不会变。把它放进 `core`，剩下的都只是壳。
