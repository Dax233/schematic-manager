# 1.20.4 物品降级校验数据

`items-1.20.4.txt` 和 `enchantments-1.20.4.txt` 是 1.20.4 原版物品、附魔标识列表，
省略 `minecraft:` 命名空间，每行一个。转换时用于识别需要兼容替换的物品和拒绝不存在的附魔，
不能用方块库代替物品注册表，因为许多物品不是方块。

数据来自 [misode/mcmeta 的 1.20.4 注册表](https://github.com/misode/mcmeta/tree/1.20.4-registries)：

- [物品注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/item/data.json)
- [附魔注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/enchantment/data.json)

按 JSON 数组顺序逐行导出，未添加或删除标识。更新支持的目标版本时，需要同时核对物品结构、
组件映射及该版本的注册表，不能仅替换这两份列表。
