# 调色板格式

> 仙剑奇侠传的 256 色调色板（Palette）

---

## 概述

仙剑使用 8 位色（8-bpp）渲染，即每个像素是 1 字节的索引值，指向一个包含 256 种颜色的调色板。调色板定义了游戏画面的所有颜色。

调色板数据存储在 `PAT.MKF` 中，每个 chunk 是一套 palette；部分 chunk 在日间颜色之后还包含 768 字节的夜间颜色。

---

## 颜色格式

每个颜色条目为 3 字节的 RGB（没有 Alpha 通道）：

```
字节 0:   Red   红色分量 (0-63)
字节 1:   Green 绿色分量 (0-63)
字节 2:   Blue  蓝色分量 (0-63)
```

> **注意**：仙剑调色板的 RGB 值范围为 **0-63**（6 位精度），而不是通常的 0-255（8 位）。渲染时需要左移 2 位（×4）映射到 0-255。

---

## 调色板结构

```
偏移量      大小      说明
────────────────────────────────────
0x000       3 bytes   颜色 0 (R, G, B)
0x003       3 bytes   颜色 1 (R, G, B)
0x006       3 bytes   颜色 2 (R, G, B)
...         ...
0x2FD       3 bytes   颜色 255 (R, G, B)
────────────────────────────────────
总计:       768 字节  (256 × 3)
```

### 颜色 0：透明色

调色板中**索引 0 通常为透明色**（黑色 #000000），在渲染时被忽略，透出背景。

---

## 在 Rust 中的数据结构

```rust
/// 一个 RGB 颜色（6 位精度）
struct PaletteColor {
    r: u8,   // 0-63
    g: u8,   // 0-63
    b: u8,   // 0-63
}

impl PaletteColor {
    /// 转换为 8 位 RGB（左移 2 位）
    fn to_rgb8(&self) -> (u8, u8, u8) {
        (self.r << 2, self.g << 2, self.b << 2)
    }
}

/// 256 色调色板
struct Palette {
    colors: [PaletteColor; 256],
}

impl Palette {
    /// 从 768 字节的原始数据加载
    fn from_bytes(data: &[u8]) -> Option<Self>;
    
    /// 获取某个索引的 8 位 RGB 值
    fn get_rgb(&self, index: u8) -> (u8, u8, u8);
    
    /// 将 8 位索引像素缓冲区转换为 RGBA 缓冲区
    fn apply(&self, pixels: &[u8]) -> Vec<[u8; 4]>;
}
```

---

## 调色板在游戏中的使用

### 调色板动画（Palette Animation）

仙剑使用调色板动画实现许多视觉效果：

- **水波纹**：通过循环偏移调色板中蓝色系颜色的索引范围
- **闪烁**：快速切换特定颜色的亮度
- **淡入淡出**：将调色板所有颜色逐渐过渡到黑色或从黑色恢复
- **昼夜切换**：整体调暗调色板

SDLPAL 中通过 `PAL_SetPalette()` 和 `PAL_FadeIn()` / `PAL_FadeOut()` 实现。

### 色移效果（Color Shift）

在战斗中使用**色移**实现伤害闪白、中毒变紫等效果：
- 对渲染的像素索引加上一个偏移值，映射到调色板的不同区域
- `PAL_RLEBlitWithColorShift()` 实现此功能

---

## 参考

- SDLPAL 参考实现：`video.c` 中的 `PAL_SetPalette()`、`PAL_GetPalette()`、`PAL_FadeIn()`、`PAL_FadeOut()`
- 调色板格式参考：[PAL Research Project](https://github.com/palxex/palresearch)
