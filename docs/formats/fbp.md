# FBP 全屏图片格式

`FBP.MKF` 保存 320×200、8 位索引色的全屏图片，主要用于剧情过场和结局。图片使用当前
palette 显示，资源层只负责返回 64000 个 palette 索引，不负责 RGBA 转换或过场时序。

## Chunk 内容

每个 MKF chunk 对应一张零基编号图片，存在两种编码：

- 长度正好为 64000 字节时，chunk 就是按行优先排列的原始索引 framebuffer。
- 其他非空 chunk 必须是 YJ_1 流；解压结果必须恰好为 64000 字节。

资源层将空 chunk、YJ_1 解压失败或解压长度不符视为不可用图片。表现层只为原版脚本
明确使用的 `SHOW_FBP 0xffff` 保留黑屏兼容行为，不把任意损坏 chunk 静默补零。

## 脚本表现

- `SHOW_FBP` 可直接替换画面，也可按 operand 指定的延时逐步淡入；它会清除此前设置的
  结局精灵。
- `SCROLL_FBP` 将新图片的底部逐行从屏幕顶端卷入，同时把旧画面向下推出；速度为 0
  时按 1 处理。
- `FBP_EFFECT` 可设置一个 `MGO.MKF` 精灵。该设置会保留到后续 FBP 淡入和滚屏，精灵
  在左上角以约 150ms 每帧播放；值 `0xffff` 表示沿用此前设置，值 0 表示清除。

原版淡入按 16×6 个索引阶段执行，每阶段等待 `(fade + 1) * 10ms`；场景淡入使用
12×6 个阶段，每阶段等待 `(speed + 1) * 10ms`。桌面层按固定更新时间步换算持续 tick，
并保持数值越大、效果越慢的语义。

## 参考

- SDLPAL `ending.c` 的 `PAL_ShowFBP`、`PAL_ScrollFBP` 和结局精灵行为
- SDLPAL `video.c` 的 `VIDEO_FadeScreen` 时序
- 本仓库 `pal-assets/src/fbp.rs` 与 `pal-desktop/src/window/visual.rs`
