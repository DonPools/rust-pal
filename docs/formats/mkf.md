# MKF 文件格式

> 仙剑奇侠传系列使用的通用资源打包格式

---

## 概述

MKF（MaKaFei）是仙剑奇侠传基础资源打包格式。所有游戏数据——图片、地图、音频、文本、动画——都存储在 MKF 包中。可以把 MKF 理解为一种**没有目录名的 ZIP 文件**，通过偏移表索引每个子文件（chunk）。

---

## 文件布局

```
偏移量          内容                    说明
─────────────────────────────────────────────────────────────
0x00            u32 table_size          索引表总字节数（小端序）
0x04            u32 offset_0            chunk 0 的绝对文件偏移
0x08            u32 offset_1            chunk 1 的绝对文件偏移
0x0C            u32 offset_2            chunk 2 的绝对文件偏移
...             ...                     ...
0x04 + N*4      u32 offset_N            chunk N 的绝对文件偏移
─────────────────────────────────────────────────────────────
offset_0        [u8; size_0]            chunk 0 数据
offset_1        [u8; size_1]            chunk 1 数据
...             ...                     ...
offset_N        [u8; size_N]            chunk N 数据
```

### 索引表详解

- `table_size`：索引表占据的总字节数，**包含自身这 4 字节**
- `chunk` 数量 = `(table_size - 4) ÷ 4`
- 每个 `offset_N` 是 chunk N 在文件中的**绝对偏移量**（从文件头 0x00 开始算）
- 索引表有 `chunk_count + 1` 个偏移值（最后一个作为结束标记）

### Chunk 大小计算

```
chunk_N 大小 = offset_{N+1} - offset_N
```

最后一个 chunk 的大小 = `文件总大小 - offset_N`

---

## 示例：ABC.MKF 头部解析

从 `ABC.MKF` 的十六进制数据：

```
偏移 0x00:  6c 02 00 00   → table_size = 0x026c = 620
偏移 0x04:  6c 02 00 00   → offset_0   = 0x026c = 620（即第一个 chunk 从文件偏移 620 处开始）
偏移 0x08:  7e 09 00 00   → offset_1   = 0x097e = 2430
偏移 0x0C:  92 0d 00 00   → offset_2   = 0x0d92 = 3474
```

计算：
- `chunk_count = (620 - 4) ÷ 4 = 154` 个 chunk
- `chunk_0 大小 = 2430 - 620 = 1810` 字节
- `chunk_1 大小 = 3474 - 2430 = 1044` 字节

---

## 在 Rust 中的数据结构

```rust
/// MKF 索引表头部
struct MkfHeader {
    table_size: u32,   // 索引表总大小（小端序）
}

/// MKF 归档
struct MkfArchive {
    offsets: Vec<u32>, // 每个 chunk 的偏移量
    data: Vec<u8>,     // 完整文件数据
}

impl MkfArchive {
    fn chunk_count(&self) -> usize;     // chunk 数量
    fn read_chunk(&self, i) -> &[u8];   // 读取第 i 个 chunk
    fn chunk_sizes(&self) -> Vec<usize>;// 每个 chunk 的大小
}
```

---

## 各 MKF 文件内容

| 文件名 | chunk 数量 | 内容说明 |
|--------|-----------|----------|
| `ABC.MKF` | ~154 | 法术/魔法动画（帧序列） |
| `BALL.MKF` | 少量 | 道具/物品图标 |
| `DATA.MKF` | 少量 | 游戏初始化参数 |
| `F.MKF` | 大量 | 角色精灵帧数据 |
| `FBP.MKF` | 中等 | 战斗背景图片 |
| `FIRE.MKF` | 中等 | 火焰/特效动画 |
| `GOP.MKF` | 大量 | 过场动画？ |
| `MAP.MKF` | 大量 | 场景瓦片地图数据 |
| `MGO.MKF` | 中等 | 迷宫全局对象 |
| `MIDI.MKF` | 几十 | 可选 Standard MIDI 背景音乐回退 |
| `MUS.MKF` | 几十 | DOS 原版 RIX / OPL2 背景音乐 |
| `PAT.MKF` | 少量 | 256 色调色板（日间及可选夜间颜色） |
| `RGM.MKF` | 大量 | 随机迷宫生成数据 |
| `RNG.MKF` | 大量 | 随机迷宫地图 |
| `SSS.MKF` | 少量 | 场景、事件对象、全局对象和脚本记录 |
| `VOC.MKF` | 大量 | Creative Voice File 音效片段 |

---

## 重要说明

1. **小端序**：所有多字节数值均为小端（Little-Endian）存储
2. **绝对偏移**：偏移量从文件头 0x00 开始计算，不是相对于索引表
3. **空 chunk**：如果 `offset_N == offset_{N+1}`，该 chunk 大小为 0（空 chunk）
4. **压缩**：chunk 内部数据可能使用 RLE 或其他压缩算法，需进一步解压
5. **不包含文件名**：MKF 格式不存储文件名信息，只能通过索引号访问

---

## 参考

- SDLPAL 参考实现：`palcommon.c` 中的 `PAL_MKFGetChunkCount()`、`PAL_MKFReadChunk()` 等函数
- [PAL Research Project](https://github.com/palxex/palresearch) — 更详细的格式文档
