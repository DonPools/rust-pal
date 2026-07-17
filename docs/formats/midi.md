# MIDI 背景音乐

## 存储

DOS 版 `MIDI.MKF` 的零基 chunk 编号就是脚本使用的音乐编号。chunk 0 为空，非空
chunk 是完整的 Standard MIDI File，以 `MThd` 头和一个或多个 `MTrk` 轨道组成。

当前解析器支持 format 0 和 format 1、ticks-per-quarter-note 时间基准、可变长整数、
running status、tempo meta event、SysEx 跳过以及常用 channel event。SMPTE 时间基准、
损坏的轨道边界、非法状态字节和超过四字节的可变长整数会被拒绝。

## 播放

桌面层使用成熟的纯 Rust `rustysynth` 后端和 General MIDI SoundFont，将完整 MIDI
事件合成为 `44100 Hz` 立体声 PCM，再交给 `rodio` 播放。该后端支持 SoundFont 采样、
包络、滤波、颤音、声像、expression、延音踏板、pitch bend、混响和 chorus，并支持
剧情脚本要求的启动、停止、循环和淡入。

SoundFont 从 `data/TimGM6mb.sf2` 加载，不嵌入 binary 或仓库。资源检查会验证 SoundFont
结构；缺少或损坏时启动会明确失败，不再回退到音色失真的简易振荡器。

脚本 `0x0043` 的音乐编号 `0` 表示停止。当前音乐编号属于核心游戏状态，并包含在开发
快照中；恢复快照时桌面层会重新开始对应曲目。

## 参考

- Standard MIDI File 1.0：`MThd`、`MTrk`、VLQ 和 running status
- SDLPAL `midi.c`：`MIDI.MKF` chunk 编号和循环行为
- RustySynth：SoundFont 2 和 Standard MIDI File 合成
