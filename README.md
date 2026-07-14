# Rust-PAL

**仙剑奇侠传 (Legend of Sword and Fairy) — Rust 复刻版**

用 Rust 重新实现经典国产 RPG《仙剑奇侠传》。基于 SDLPAL 的 C 代码作为参考，完全从零用 Rust 构建。

---

## 目录结构

```
rust-pal/
├── Cargo.toml                  # Cargo workspace
├── README.md
├── .gitignore
│
├── pal-assets/                 # 资源加载与格式解析
│   ├── Cargo.toml
│   └── src/lib.rs
│
├── pal-core/                   # 核心游戏引擎（场景、战斗、背包、剧情）
│   ├── Cargo.toml
│   └── src/lib.rs
│
├── pal-desktop/                # 桌面端渲染与交互（窗口、输入、音频）
│   ├── Cargo.toml
│   └── src/lib.rs
│
├── pal-launcher/               # 游戏启动入口（binary）
│   ├── Cargo.toml
│   └── src/lib.rs
│
├── data/                       # 游戏原始数据 (MKF 资源文件，已 gitignore)
│   ├── ABC.MKF
│   ├── DATA.MKF
│   ├── MAP.MKF
│   ├── MIDI.MKF
│   └── ...
│
└── reference/                  # 参考实现（已 gitignore）
    ├── sdlpal/                 # C 版 SDLPAL
    └── jsdos/                  # 网页 DOSBox 配置
```

---

## 技术栈

| 层级 | Crate | 关键依赖 | 职责 |
|------|-------|----------|------|
| 启动入口 | `pal-launcher` | `pal-desktop` | 解析命令行、初始化、启动游戏 |
| 窗口/渲染 | `pal-desktop` | `winit` + `pixels` + `rodio` | 窗口管理、像素渲染、音频播放、输入处理 |
| 游戏逻辑 | `pal-core` | `glam` (不依赖其他) | 场景、战斗、对话、背包等游戏状态机 |
| 资源解析 | `pal-assets` | `binrw` | MKF 包解析、RLE 解压、PNG/WAV 解码 |

### 选型理由

- **`winit`** — Rust 生态最轻量的跨平台窗口库，无引擎包袱
- **`pixels`** — 直接操作像素缓冲区，适合 256 色 palette 的逐帧渲染
- **`rodio`** — 最简单的跨平台音频播放库，支持 WAV/OGG/Vorbis
- **`binrw`** — 声明式二进制结构体解析，一行代码定义一个 MKF header
- **`glam`** — 轻量 2D 数学库，游戏逻辑中处理坐标、碰撞等

---

## 构建与运行

### 前置条件

```shell
# macOS
brew install sdl3       # 可选，仅编译 reference/sdlpal 时需要

# Linux
sudo apt install libsdl2-dev  # 同上
```

### 编译

```shell
cargo build
```

### 运行

游戏数据放在 `data/` 目录下（已提供）。直接运行：

```shell
cargo run -p pal-launcher
```

也可用 reference 的 SDLPAL 运行：

```shell
cd data && ./sdlpal
```

---

## 开发路线

### Phase 1 — 资源层 ✅ 已完成
- [x] 项目骨架搭建
- [x] Cargo workspace 结构
- [x] MKF 包解析器 (pal-assets)
- [ ] RLE 解压缩
- [ ] 调色板 (palette) 解析
- [ ] 位图 (bitmap) 解码

### Phase 2 — 显示层
- [ ] winit 窗口 + pixels 像素缓冲区
- [ ] 256 色 palette 渲染管线
- [ ] 场景瓦片地图渲染
- [ ] 角色精灵渲染

### Phase 3 — 交互层
- [ ] 键盘输入处理 (方向键 + 确认/取消)
- [ ] 角色移动与碰撞检测
- [ ] BGM 播放 (MIDI/WAV)

### Phase 4 — 游戏逻辑
- [ ] 剧情脚本引擎 (M.MSG)
- [ ] 对话系统
- [ ] 战斗系统
- [ ] 背包与道具

---

## 参考资源

- [SDLPAL (C 版参考实现)](https://github.com/sdlpal/sdlpal) — 位于 `reference/sdlpal/`
- [PAL Research Project](https://github.com/palxex/palresearch) — 仙剑数据格式文档
- 原始游戏数据版权归 **软星科技 (SoftStar Inc.)** 所有

---

## 许可

本项目代码采用 **GPL v3** 许可。
不包含任何原始游戏代码或数据文件。
