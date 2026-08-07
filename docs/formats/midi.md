# MIDI 兼容回退

## 存储

`MIDI.MKF` 的零基 chunk 编号与脚本音乐编号一致。chunk 0 为空，非空 chunk 是完整的
Standard MIDI File，以 `MThd` 头和一个或多个 `MTrk` 轨道组成。它不是 DOS 原版的主
音乐路径；桌面端优先读取同编号的 `MUS.MKF` RIX，只有 RIX 缺失或无法准备时才尝试 MIDI。

当前解析器支持 format 0 和 format 1、ticks-per-quarter-note 时间基准、可变长整数、
running status、tempo meta event、SysEx 跳过以及常用 channel event。SMPTE 时间基准、
损坏的轨道边界、非法状态字节和超过四字节的可变长整数会被拒绝。

## 播放

兼容回退使用纯 Rust `rustysynth` 和 General MIDI SoundFont，将完整 MIDI 事件合成为
`44100 Hz` 立体声 PCM，再交给 `rodio` 播放。该后端支持 SoundFont 采样、
包络、滤波、颤音、声像、expression、延音踏板、pitch bend、混响和 chorus，并支持
剧情脚本要求的启动、停止、循环和淡入。同一曲目再次收到播放指令时会原地更新循环
标志，不从头重播；每一遍结束时根据最新标志决定继续或在两秒 release tail 后停止。

系统菜单使用“禁用 / 启用”开关。内部音乐增益默认 100%；MIDI 各通道的 controller 7、
expression 和 velocity 仍由 SoundFont 合成器独立处理。真实资源检查在回退资源存在时
统计多通道歌曲、通道音量事件并验证实际合成输出。

SoundFont 默认从 `data/TimGM6mb.sf2` 加载，不嵌入 binary 或仓库。`MIDI.MKF` 与
SoundFont 都是可选资源；缺少时 DOS RIX/OPL2 播放和游戏启动不受影响。

脚本 `0x0043` 的音乐编号 `0` 表示停止。当前音乐编号属于核心游戏状态，并包含在开发
快照中；恢复快照时桌面层会重新开始对应曲目。

## 参考

- Standard MIDI File 1.0：`MThd`、`MTrk`、VLQ 和 running status
- SDLPAL `midi.c`：`MIDI.MKF` 兼容 chunk 编号和循环行为
- RustySynth：SoundFont 2 和 Standard MIDI File 合成
