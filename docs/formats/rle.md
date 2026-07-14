# RLE 位图格式

> 仙剑奇侠传中精灵（Sprite）和位图使用的 RLE 压缩格式

---

## 概述

仙剑的所有角色精灵、物品图标、瓦片贴图都使用一种简单的 RLE（Run-Length Encoding）压缩格式存储。这是 8-bpp（8 位每像素，256 色）的位图，用索引色值指向调色板。

数据通常在 MKF chunk 内，也可能直接以 `.rle` 或内嵌方式存在。

---

## 文件布局

```
偏移量      内容                     说明
──────────────────────────────────────────────────────
0x00        u32 magic (可选)         固定值 0x00000002（不一定存在）
0x00/0x04   u16 width                位图宽度（像素，小端序）
0x02/0x06   u16 height               位图高度（像素，小端序）
0x04/0x08   ...                      RLE 压缩的像素数据
```

### 头部说明

- **Magic 字**：如果前 4 字节为 `0x00000002`，则跳过这 4 字节再读取宽高
- **Width**：位图宽度，小端序 u16，范围通常 16~640
- **Height**：位图高度，小端序 u16

### 像素数据存储方式

像素数据按**行优先**（row-major）顺序存储，每像素 1 字节（8 位索引色）。数据使用 RLE 压缩：

```
每个命令占用 1 字节 + N 字节数据

命令字节 T：
  ├── T > 0x80 且 T <= 0x80 + width：
  │     透明（跳过）命令
  │     跳过 (T - 0x80) 个像素（保持原背景）
  │     不消耗额外字节
  │
  └── T <= 0x80：
         非透明（绘制）命令
         紧接着 T 个像素字节（索引色值）
         每个像素 1 字节，共 T 字节数据
```

### 解码算法伪代码

```
i = 0        // 已处理像素计数
src_x = 0   // 当前行内 x 偏移
y = 0       // 当前行号
width, height = 从头部读取

while i < width * height:
    T = read_byte()
    
    if T > 0x80 and T <= 0x80 + width:
        // 透明跳过
        i += T - 0x80
        src_x += T - 0x80
        if src_x >= width:
            src_x -= width
            y += 1
    else:
        // 绘制 T 个像素
        for j in 0..T:
            pixel = read_byte()
            if 像素在显示范围内:
                写入像素(x, y)
            src_x += 1
            if src_x >= width:
                src_x = 0
                y += 1
        i += T
```

---

## 在 Rust 中的数据结构

```rust
/// RLE 压缩位图
struct RleBitmap {
    width: u16,           // 位图宽度
    height: u16,          // 位图高度
    data: Vec<u8>,        // 解码后的像素数据（width * height 字节）
}

impl RleBitmap {
    /// 从原始 RLE 数据解码
    fn decode(raw: &[u8]) -> Option<Self>;
    
    /// 获取指定坐标的像素索引值
    fn get_pixel(&self, x: u16, y: u16) -> Option<u8>;
    
    /// 将位图渲染到目标缓冲区
    fn blit_to(&self, target: &mut [u8], target_w: u16, x: i16, y: i16);
}
```

---

## 示例

以下是一个 4x3 像素的 RLE 数据示意：

```
原始 RLE 数据：
  03  AA BB CC    → 绘制 3 个像素（AA BB CC）
  83              → 跳过 3 个像素（透明）
  01 DD           → 绘制 1 个像素（DD）
  ...

解码后（假设 palette 有颜色）：
  AA BB CC ..    （第 0 行）
  .. DD .. ..    （第 1 行）
  .. .. .. ..    （第 2 行）
```

---

## 参考

- SDLPAL 参考实现：`palcommon.c` 中 `PAL_RLEBlitToSurface()`、`PAL_RLEGetWidth()`、`PAL_RLEGetHeight()`
- 数据存储在 `F.MKF`（角色精灵）、`FBP.MKF`（战斗背景）、`BALL.MKF`（物品图标）等 MKF 包中
