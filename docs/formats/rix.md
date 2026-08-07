# RIX / OPL2 背景音乐

## MUS.MKF 与曲目头

DOS 原版背景音乐来自 `MUS.MKF`。MKF 的零基 chunk 编号就是脚本音乐编号；chunk 0
为空。每个非空 chunk 是一首 Softstar RIX 曲目，至少包含 16 字节头：

| 偏移 | 类型 | 内容 |
|---:|---|---|
| `0x00` | `u16` | little-endian 魔数 `0x55AA`（文件字节为 `AA 55`） |
| `0x02` | `u8` | 非零表示启用 OPL2 rhythm/percussion 模式 |
| `0x08` | `u16` | 乐器表在当前 chunk 内的偏移 |
| `0x0C` | `u16` | 音乐命令流在当前 chunk 内的偏移 |

偏移必须落在 chunk 内，命令流还必须至少能读取一对字节。乐器编号 `n` 从
`instrument_offset + n * 64` 开始读取前 56 字节，即 28 个 little-endian word；它们
描述两个 operator 的波形、包络、倍频、反馈和电平。任何截断乐器、越界命令或非法通道
都会让该曲目准备失败，不能 panic。

## 命令流

命令以 `[value, control]` 两字节为一组。`control` 高四位决定操作，低四位是通道：

| 高四位 | 行为 |
|---:|---|
| `0x90` | 选择 `value` 指定的乐器并写入对应 operator 寄存器 |
| `0xA0` | 设置通道 pitch bend，并立即重写当前音高 |
| `0xB0` | 将 `value` 夹紧到 `0x7F` 后设置通道音量 |
| `0xC0` | 先 key-off；`value != 0` 时再以该音符 key-on |
| 其他 | 两字节整体作为延时值，进入下一次时钟推进 |

`control == 0x80` 是曲目结束：关闭全部声道，并把命令位置复位到开头。旋律模式使用
六个双 operator 通道；rhythm 模式还按 OPL2 `0xBD` 的五个 percussion bit 驱动鼓点。

## 播放时钟与输出

RIX 驱动按原版 70 Hz 推进，每 tick 从延时累计中减 14。桌面端固定以 `44100 Hz`
生成音频，因此每 tick 恰好生成 630 个立体声 frame。命令被翻译成 OPL2 寄存器写入，
再交给纯 Rust Nuked-OPL3 内核在 OPL2 模式合成 `i16` PCM，最终由 `rodio` 播放。

循环不是预先展开曲目：到结束标记时重新初始化寄存器和命令流。正在播放同一编号的曲目
再次收到脚本请求时，只更新共享循环标志；每遍边界读取最新值。单遍最多允许 15 分钟，
立即结束或损坏的曲目不会形成忙循环。

桌面端优先使用 `MUS.MKF`。只有对应 RIX chunk 缺失或无法准备时，才尝试同编号的
`MIDI.MKF` + SoundFont 兼容回退。脚本音乐编号 0 表示停止；系统菜单则提供整路音乐的
“禁用 / 启用”开关。

真实资源检查会解析并有界执行全部非空 RIX chunk、统计寄存器写入，并选择多首曲目实际
合成 PCM，防止“格式能读但输出静音”。

## 参考

- SDLPAL Classic `rixplay.cpp`、`adplug/rix.cpp`：RIX 时钟、命令和寄存器映射
- Yamaha YM3812 / OPL2 寄存器定义
