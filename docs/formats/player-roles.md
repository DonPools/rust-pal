# 玩家角色数据

## 概述

`DATA.MKF` 的零基 chunk 3 以 `PLAYERROLES` 表开头。表中的数值均为小端
`u16`，每个 `PLAYERS` 数组包含 6 个角色值。M1 当前只读取场景角色渲染所需的
两个数组，其他属性留到实际玩法需要时再解析。

## 场景图形字段

| 数组序号 | 字节偏移 | 字段 | 含义 |
|---:|---:|---|---|
| 2 | 24 | `rgwSpriteNum` | 角色在 `MGO.MKF` 中的场景精灵 chunk 索引 |
| 64 | 768 | `rgwWalkFrames` | 每个方向的步行动画帧数 |

角色 `n` 在数组内的地址为 `数组偏移 + n * 2`，有效角色索引是 `0..6`。
`rgwWalkFrames` 为 4 时使用每方向 4 帧；其他值通常按每方向 3 帧处理，值为 0
时参考实现也回退到 3 帧。

场景精灵按南、西、北、东四个方向连续存放，因此绝对帧索引为：

```text
frame = direction * walk_frames + animation_frame
```

## 边界要求

- chunk 截断或角色索引越界时解析失败。
- `rgwSpriteNum` 必须保留原始 MKF 零基索引。
- 使用精灵前应验证四个方向所需帧均存在且能解码。

## 参考

- SDLPAL `global.h`：`PLAYERROLES` 布局
- SDLPAL `scene.c`：`rgwWalkFrames` 的帧索引和 3 帧回退规则
