# 玩家角色数据

## 概述

`DATA.MKF` 的零基 chunk 3 以 `PLAYERROLES` 表开头。表中的数值均为小端
`u16`，每个 `PLAYERS` 数组包含 6 个角色值。当前原版 DOS 数据中的表长为
900 字节，即 75 个连续数组。

角色 `n` 在数组内的地址为：

```text
array_index * 12 + n * 2
```

有效角色索引是 `0..6`。装备栏、元素抗性和法术栏仍采用按字段分组的结构，
也就是先存所有角色的第一个值，再存所有角色的第二个值。

## 已解析字段

| 数组序号 | 字段 | 含义 |
|---:|---|---|
| 0 | `rgwAvatar` | 状态界面头像 |
| 1 | `rgwSpriteNumInBattle` | `F.MKF` 战斗精灵 |
| 2 | `rgwSpriteNum` | `MGO.MKF` 场景精灵 |
| 3 | `rgwName` | `WORD.DAT` 名称索引 |
| 4 | `rgwAttackAll` | 普通攻击是否作用于全体 |
| 6 | `rgwLevel` | 初始等级 |
| 7..10 | `rgwMaxHP` .. `rgwMP` | HP、MP 上限和当前值 |
| 11..16 | `rgwEquipment` | 6 个装备栏 |
| 17..22 | 基础战斗属性 | 攻击、灵力、防御、身法、吉运和毒抗 |
| 23..27 | `rgwElementalResistance` | 5 种元素抗性 |
| 31 | `rgwCoveredBy` | 濒危时援护角色 |
| 32..63 | `rgwMagic` | 32 个法术栏 |
| 64 | `rgwWalkFrames` | 每方向场景步行动画帧数 |

`rgwWalkFrames` 为 4 时使用每方向 4 帧；其他值按每方向 3 帧处理，值为 0
时也回退到 3 帧。场景精灵按南、西、北、东四个方向连续存放：

```text
frame = direction * walk_frames + animation_frame
```

## 边界要求

- chunk 不足 900 字节或角色索引越界时解析失败。
- 所有资源编号和文字索引保留原始零基值，使用时再验证目标资源存在。
- 使用精灵前验证四个方向所需帧均存在且能解码。
- 队伍最多包含 5 个不同角色；这是运行时约束，不属于资源表布局。

运行时持有完整六角色可变表；队伍成员属性是该表的同步视图。HP、MP、装备和精灵等
变化不会因角色离队再入队而重置，并随版本 9 开发快照一起保存。

## 参考

- SDLPAL `global.h`：`PLAYERROLES`、`PLAYERS` 布局
- SDLPAL `global.c`：默认角色属性加载
- SDLPAL `scene.c`：步行帧索引和 3 帧回退规则
