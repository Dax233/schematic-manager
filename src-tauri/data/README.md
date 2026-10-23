# 1.20.4 物品降级校验数据

`items-1.20.4.txt` 和 `enchantments-1.20.4.txt` 是 1.20.4 原版物品、附魔标识列表，
省略 `minecraft:` 命名空间，每行一个。转换时用于识别需要兼容替换的物品和拒绝不存在的附魔，
不能用方块库代替物品注册表，因为许多物品不是方块。

数据来自 [misode/mcmeta 的 1.20.4 注册表](https://github.com/misode/mcmeta/tree/1.20.4-registries)：

- [物品注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/item/data.json)
- [附魔注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/enchantment/data.json)

按 JSON 数组顺序逐行导出，未添加或删除标识。更新支持的目标版本时，需要同时核对物品结构、
组件映射及该版本的注册表，不能仅替换这两份列表。

## 方块注册与状态信息

`block-registry.json.gz` 独立记录精确数据版本下的原版方块名称、完整属性域和默认值。
渲染模型不包含完整游戏状态，且特殊渲染方块可能没有模型几何，不能据此判断方块是否存在。
应用离线读取这份数据；加载旧基础库时只在内存补齐注册名，不修改磁盘缓存，也不制造渲染模型。
转换用目标版本属性保留合法源属性并补齐默认值，模组方块继续使用目标资源包提供的信息。

来源为 [misode/mcmeta](https://github.com/misode/mcmeta) 的每个正式版本
`<version>-registries/block/data.json` 和 `<version>-summary/blocks/data.json`。
注册表包含无属性方块；摘要的第一项为属性域、第二项为游戏默认状态，未按模型变体顺序推断。
`block-registry-sources.json` 记录各版本的数据版本号、输入 URL 与 SHA256。
相同内容在压缩数据中去重，查询仍严格按数据版本号匹配。

覆盖 `base_db::SUPPORTED_RELEASES` 中 1.14 起的版本；上游缺少完整 1.13.2 摘要，
因此该版本仍可建库/预览，但不执行缺少精确属性定义的转换，也不使用邻近版本代替。
更新时在仓库根目录运行 `python tools/update_block_registry.py`；任何下载或数据校验失败
都会中止生成，不写出缺少版本的部分结果。
