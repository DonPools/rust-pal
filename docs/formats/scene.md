# 场景与事件对象格式

## 概述

`SSS.MKF` 保存场景、事件对象、全局对象和脚本等数据。M2 当前解析零基 chunk 0
和 chunk 1；其他 chunk 等到脚本运行时需要时再扩展。

| Chunk | 记录大小 | 内容 |
|---:|---:|---|
| 0 | 32 字节 | 全局事件对象表 |
| 1 | 8 字节 | 场景表 |
| 2 | 版本相关 | 全局对象表 |
| 3 | - | 消息索引等辅助数据 |
| 4 | 8 字节 | 脚本记录 |

所有字段均为 little-endian 16 位整数。

## 场景记录

| 偏移 | 类型 | 字段 | 含义 |
|---:|---|---|---|
| 0 | `u16` | `map_num` | `MAP.MKF`/`GOP.MKF` 使用的一基地图编号 |
| 2 | `u16` | `script_on_enter` | 进入场景脚本入口 |
| 4 | `u16` | `script_on_teleport` | 离开场景脚本入口 |
| 6 | `u16` | `event_object_index` | 全局事件对象表中的零基边界 |

游戏场景编号从 1 开始。场景 `n` 的事件对象范围是：

```text
scenes[n - 1].event_object_index .. scenes[n].event_object_index
```

因此选择一个场景时必须存在下一条场景记录。解析器要求边界单调不减且不能超过
chunk 0 的事件对象总数。

## 事件对象记录

| 偏移 | 类型 | 字段 | 含义 |
|---:|---|---|---|
| 0 | `i16` | `vanish_time` | 暂时消失计时 |
| 2 | `u16` | `x` | 地图逻辑 X 坐标 |
| 4 | `u16` | `y` | 地图逻辑 Y 坐标 |
| 6 | `i16` | `layer` | 绘制层高度 |
| 8 | `u16` | `trigger_script` | 触发脚本入口 |
| 10 | `u16` | `auto_script` | 自动脚本入口 |
| 12 | `i16` | `state` | 0 隐藏、1 普通、2 及以上参与阻挡 |
| 14 | `u16` | `trigger_mode` | 0 无、1..3 面前交互、4..8 接触触发 |
| 16 | `u16` | `sprite_num` | `MGO.MKF` chunk；0 表示无精灵 |
| 18 | `u16` | `sprite_frames` | 每方向动画帧数 |
| 20 | `u16` | `direction` | 南、西、北、东方向值 0..3 |
| 22 | `u16` | `current_frame` | 当前动画帧 |
| 24 | `u16` | `script_idle_frame` | 触发脚本空闲计数 |
| 26 | `u16` | `sprite_ptr_offset` | 兼容字段，语义待确认 |
| 28 | `u16` | `auto_sprite_frames` | 自动脚本使用的精灵总帧数 |
| 30 | `u16` | `auto_script_idle_frame` | 自动脚本空闲计数 |

## 边界要求

- chunk 长度必须是对应记录大小的整数倍。
- 场景表至少包含一条可选择记录及其下一条边界记录。
- 事件范围必须单调且位于事件对象表内。
- 使用 `sprite_num` 前仍需验证对应 `MGO.MKF` chunk 存在并可解码。

## 运行时规则

- `state <= 0`、`vanish_time > 0` 或无精灵的对象不绘制。
- `state >= 2` 的对象参与碰撞；距离判定为
  `abs(object_x - x) + abs(object_y - y) * 2 < 16`。
- `sprite_frames > 0` 时，帧索引按 `direction * sprite_frames + frame` 计算。
  三帧步行动画的运行帧 2、3 分别映射到资源帧 0、2。
- `sprite_frames == 0` 时，当前帧直接索引 GOP；实际总帧数由对应 `MGO.MKF`
  sprite 在加载场景时补充。
- 事件对象绘制位置、逻辑层和覆盖 tile 参与统一的深度排序。
- 接触模式 4..8 的触发距离依次为 16、48、80、112、144，距离使用与碰撞相同
  的压缩菱形度量。
- 面前交互按玩家朝向生成 13 个检查点；搜索模式 1、2、3 分别覆盖前 1、2、3
  格，并按检查点和事件对象原始顺序选择首个匹配对象。
- 触发检测只生成包含事件对象 ID 和脚本入口的待处理请求；脚本运行时负责消费请求
  并回写后续脚本入口。

## 参考

- SDLPAL `global.h`：`SCENE`、`EVENTOBJECT` 和状态/触发模式常量
- SDLPAL `global.c`：`SSS.MKF` chunk 装载方式
- SDLPAL `res.c`、`scene.c`：当前场景事件范围与精灵使用方式
