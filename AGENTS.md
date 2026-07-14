# AGENTS.md

本文件适用于整个仓库。开始修改前先阅读本文件、`README.md` 以及与任务相关的 `docs/formats/` 文档。

## 项目概览

Rust-PAL 是经典 RPG《仙剑奇侠传》的 Rust 重实现，目前处于资源解析、地图加载和桌面渲染管线的早期开发阶段。仓库是 Rust 2021 Cargo workspace：

- `pal-assets`：MKF、palette、RLE bitmap、GOP sprite 和纯 Rust YJ_1 等二进制资源解析。
- `pal-core`：平台无关游戏逻辑；目前主要包含地图加载、tile 索引和阻挡信息。
- `pal-desktop`：`winit` + `pixels` 桌面窗口、RGBA framebuffer 和地图渲染。
- `pal-launcher`：读取游戏数据、创建地图和渲染器并启动窗口。
- `docs/formats`：已研究的 PAL 文件格式说明。修改解析逻辑前先核对对应文档。
- `data`：本地原版游戏资源，受版权保护且被 gitignore；不得提交。
- `reference`：本地参考实现，主要是 SDLPAL；只用于查证行为，不得直接复制、批量修改或提交。

依赖方向保持为：`pal-launcher -> pal-desktop/pal-core/pal-assets`，`pal-desktop -> pal-core/pal-assets`，`pal-core -> pal-assets`。不要让资源层或核心逻辑反向依赖桌面 UI。

## 当前实现约束

- 内部渲染分辨率为 `320x200`，屏幕缓冲区使用 RGBA，每像素 4 字节。
- 原始图像为 256 色索引图；palette 索引 `0` 作为透明色。保持索引数据和 RGBA 转换的职责分离。
- PAL 文件中的整数和偏移通常为 little-endian。解析外部数据时验证长度、偏移、索引和算术边界，格式错误返回 `None`/`Result`，不能 panic。
- MKF chunk 使用零基索引；`Map::load` 的地图编号从 `1` 开始。
- 地图固定为 `128 x 64 x 2` 的交错等距 tile 布局。tile 位域、屏幕坐标和绘制顺序见 `docs/formats/map.md` 及 `pal-core/src/map.rs`。
- `pal-assets/src/yj1.rs` 是带边界检查的纯 Rust YJ_1 解压器。不要重新引入对 `reference/` 中预编译对象或 C FFI 的构建依赖。
- `pal-launcher` 从 workspace 根目录下的 `data/` 读取 `PAT.MKF`、`MAP.MKF` 和 `GOP.MKF`。不要把版权资源嵌入 binary、测试 fixture 或提交内容。

代码现状可能领先于 `README.md` 的开发路线；判断真实行为时以当前代码和测试为准，并在功能阶段明显变化时同步 README。

## 开发方式

- 通用资源解码放 `pal-assets`，平台无关状态和规则放 `pal-core`，窗口/输入/音频/显示适配放 `pal-desktop`，启动编排放 `pal-launcher`。
- 优先沿用现有模块和直接的数据结构。只有在消除实际重复或明确隔离边界时才新增抽象或依赖。
- 公共 API 和非直观的格式算法使用简洁 Rust doc comments；复杂位域或偏移算法注明格式依据。
- 用户可见文本和项目文档可使用中文；Rust 标识符使用英文。
- 工作区可能有未提交改动。修改前检查 `git status --short`，保留非本任务产生的改动。
- 不要编辑或提交 `target/`、`data/`、`reference/`、`.DS_Store` 及其他生成或忽略内容。
- 新增依赖前确认它属于正确 crate，并优先使用现有依赖或标准库。

## 测试要求

从 workspace 根目录执行：

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

真实资源的无窗口集成检查：

```sh
cargo run -p pal-launcher -- --check-assets
```

验证时注意：

- `pal-assets` 的单元测试使用内存 fixture，应保持快速且不依赖版权数据。
- `--check-assets` 依赖本地 `data/`，会验证 palette、YJ_1 地图、GOP sprite 和非空 framebuffer，但不会打开窗口。
- `cargo run -p pal-launcher` 会读取本地资源并打开窗口，属于手工 smoke test，不应作为无头 CI 的默认验证。
- 修改二进制解析器时，至少覆盖有效最小输入、截断输入、非法偏移/长度、越界索引和空 chunk。
- 修改渲染代码时，验证负坐标裁剪、viewport 边界、透明索引和 framebuffer 长度。

## 完成标准

提交结果前检查 crate 边界、格式文档、针对性测试、格式化和 Clippy。任何因缺少 Rust 工具链、游戏数据或图形环境而无法完成的验证，都要在最终说明中列出。
