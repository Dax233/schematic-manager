# 1.20.4 降级校验数据

`items-1.20.4.txt` 和 `enchantments-1.20.4.txt` 是 1.20.4 原版物品、附魔标识列表，
省略 `minecraft:` 命名空间，每行一个。转换时用于识别需要兼容替换的物品和拒绝不存在的附魔，
不能用方块库代替物品注册表，因为许多物品不是方块。

数据来自 [misode/mcmeta 的 1.20.4 注册表](https://github.com/misode/mcmeta/tree/1.20.4-registries)：

- [物品注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/item/data.json)
- [附魔注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/enchantment/data.json)
- [实体注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/entity_type/data.json)：
  `entities-1.20.4.txt` 用于转换船类型后校验顶层实体和乘客，避免新版实体被旧游戏直接丢弃。

按 JSON 数组顺序逐行导出，未添加或删除标识。更新支持的目标版本时，需要同时核对物品结构、
组件映射及该版本的注册表，不能仅替换这些列表。

跨物品组件格式的降级支持 1.20.5–1.21.4 → 1.20.4，遇到不能等价表达的组件或实体行为
会在保存前报告位置并拒绝转换。关联字段变化依据 Mojang 的
[1.21](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21)、
[1.21.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-2)、
[1.21.4](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-4) 更新说明：
迁移熔炉计时、TNT 矿车引信、原有材质的船类型和可表示为整数的模型数据；
新版实体属性修饰符、火球加速度、新版容器锁、燃烧中的熔炉总时长、非默认 TNT 行为和多通道模型数据不作有损近似。
对 1.20.5 起改名或改结构但尚未支持的旗帜图案、头颅档案、蜂巢、药水实体、坐标和拴绳字段，
也会按字段路径拒绝；刷怪笼的实体子树递归迁移，新装备战利品表拒绝。
这些变化依据 [1.20.5 更新说明](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-5)。
方块实体中的 `minecraft:custom_data` 仅作为任意元数据保留，不用其中的历史备份覆盖当前库存；
其他方块实体组件拒绝。输出文件保留这些元数据不表示 1.20.4 游戏加载、再保存后仍会保留它们。

`attributes-1.20.4.txt` 来自同版本的
[属性注册表](https://github.com/misode/mcmeta/blob/1.20.4-registries/attribute/data.json)。
无修饰符的基础属性还原为 `Attributes/Name/Base`；1.21.2 起省略的属性类型前缀由目标清单恢复，
不存在的属性仍拒绝。物品保留不可破坏标记及提示显隐（合并 `HideFlags` 的位4），
将 `sweeping_edge` 还原为旧附魔 `sweeping`，将书与笔的原始/过滤后页面还原为
`pages/filtered_pages`，不解析或改写纯文本内容。以上处理不扩大源、目标版本范围。

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

普通方块替换与版本转换共用完整属性域；开启继承时保留持久、含水等合法状态。
缺失物品和实体与方块分别列出并校验用户选择。新版唱片固定使用命名的经验修补红色玻璃板；苍白木船实体允许选择旧版木种并迁移库存和乘客。未知实体及缺少库存迁移规则的缺失容器物品明确拒绝，不静默丢弃内容。

## 容器物品的最大堆叠数
item-stacks.json.gz 来自 PrismarineJS/minecraft-data（MIT，许可见 item-stacks-LICENSE.txt），使用 dataPaths.json 明确指定的各版本物品表；来源 URL、SHA256 和缺失版本见 item-stacks-sources.json。tools/update_item_stacks.py 可重新生成。仅匹配精确数据版本，未知物品/版本的盒数显示未知，不套用相邻版本。物品自身的 minecraft:max_stack_size 组件优先。盒数为总数除以27格容量的理论折算，按物品ID合并，不考虑不同附魔/名称不能合堆的实际装箱差异。
