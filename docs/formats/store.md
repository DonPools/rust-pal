# 商店数据

## 存储

`DATA.MKF` 零基 chunk 0 是连续的商店记录。每条记录固定为 18 字节，由 9 个
little-endian `u16` 物品对象 ID 组成：

```text
struct STORE {
    items: [u16; 9]
}
```

物品 ID `0` 是列表终止符，终止符后的槽位不参与菜单。脚本 `0x0026` 的 operand 0
是零基商店编号；物品名称来自 `WORD.DAT` 的同 ID 词条，价格和 flags 来自
`SSS.MKF` chunk 2 的全局对象表。

## 交易规则

- 买入使用物品原价，金钱不足或单项达到 99 时不修改状态。
- 卖出只列出 flags 包含 bit 5 (`kItemFlagSellable`) 的背包物品。
- 卖价为原价整数除以 2。
- 已装备物品计入脚本物品总数，但不会直接出现在卖出菜单中。

## 边界要求

- chunk 不能为空且长度必须是 18 的整数倍。
- 使用商店、物品名称、价格或 flags 前分别验证对应索引。
- 交易失败必须保持金钱和背包不变。

## 参考

- SDLPAL `global.h`：`STORE`、`MAX_STORE_ITEM` 和 `ITEMFLAG`
- SDLPAL `global.c`：`DATA.MKF` chunk 0 加载
- SDLPAL `uigame.c`：`PAL_BuyMenu`、`PAL_SellMenu`
