/**
 * 方块名 → 物品键：同一个物品的不同方块形态归到同一个键上。
 *
 * Minecraft 里有些「方块」其实是同一个物品的另一种摆法，方块名却不同：
 * 火把与墙上的火把是两个方块名，可玩家要备的就是「火把」这一种物品。
 * 统计材料用量时必须按物品合计，否则同一件东西会被算成两项。
 * （Litematica 的材料清单也是按物品算的。）
 *
 * 当前覆盖的族（都在方块库里逐条核对过）：
 *
 *   1. **墙挂形态**：`torch`/`wall_torch`、`redstone_torch`/`redstone_wall_torch`、
 *      `<木种>_sign`/`<木种>_wall_sign`、`<木种>_hanging_sign`/`<木种>_wall_hanging_sign`、
 *      `<颜色>_banner`/`<颜色>_wall_banner`、`<种类>_skull`/`<种类>_wall_skull`、
 *      `<种类>_head`/`<种类>_wall_head`、`<种类>_fan`/`<种类>_wall_fan`。
 *      墙挂形态除火把外都带前缀，因此按「前缀 + 后缀」匹配（裸名只认 `wall_torch`），
 *      不做子串替换——模组方块里 `wall_` 可能本来就是名字的一部分（例如
 *      `mod:wall_panel`、`mod:wall_sign`），子串替换会把它们并到别的名字上。
 *   2. **炼药锅的内容物**：装水/岩浆/细雪的炼药锅与空炼药锅是同一个物品
 *      （游戏里只有一个「炼药锅」物品，内容物是放进去的东西）。
 *   3. **铁砧的破损程度**：`chipped_anvil`/`damaged_anvil` 与 `anvil`
 *      是同一个物品（只有一个「铁砧」物品，裂痕是使用痕迹）。
 *
 * 这几族之外还存在同类情形（例如 `potted_*` 与花盆、`kelp_plant` 与 `kelp`），
 * 但它们的对应关系不是一条名字规律，需要逐条确认，因此暂不归并：宁可按方块名
 * 分开显示，也不要把两种东西算成一种。
 *
 * 刻意**不**归并的情况，避免误合并：
 *   - `cobblestone_wall`、`stone_brick_wall` 这类真正的墙方块：名字以 `_wall`
 *     结尾而不是含 `wall_`，本函数不会动它们；
 *   - `waxed_lightning_rod` 与 `lightning_rod`：涂蜡前后是两个物品；
 *   - 同一方块的上下两半（`oak_door` 的 upper/lower）名字相同，按状态合计即可。
 */

/**
 * 唯一没有前缀的墙挂形态：火把。
 *
 * 游戏里它就叫 `wall_torch`（没有「木种」之类的前缀），其余墙挂形态都是
 * `<前缀>_wall_<形态>`。因此裸名只认这一个，免得把模组自己的方块
 * （例如 `mod:wall_sign`）并到 `mod:sign` 上。
 */
const BARE_WALL_FORM = "wall_torch";

/** 墙挂形态的后缀：命中后把这些后缀里的 `wall_` 去掉就是落地/立地形态 */
const WALL_FORM_SUFFIXES = [
  "wall_torch",
  "wall_sign",
  "wall_hanging_sign",
  "wall_banner",
  "wall_skull",
  "wall_head",
  "wall_fan",
];

/** 内容物不同、物品相同的炼药锅 */
const CAULDRON_VARIANTS = ["water_cauldron", "lava_cauldron", "powder_snow_cauldron"];

/** 破损程度不同、物品相同的铁砧 */
const ANVIL_VARIANTS = ["chipped_anvil", "damaged_anvil"];

/**
 * 取方块名对应的物品键：同一个物品的不同方块形态返回同一个值。
 *
 * 没有归并规律可用的方块原样返回，因此这个键对普通方块就是方块名本身。
 */
export function itemKeyOf(blockName: string): string {
  const colon = blockName.indexOf(":");
  const ns = colon >= 0 ? blockName.slice(0, colon + 1) : "";
  const path = colon >= 0 ? blockName.slice(colon + 1) : blockName;

  if (path === BARE_WALL_FORM) {
    return ns + BARE_WALL_FORM.replace("wall_", "");
  }
  for (const suffix of WALL_FORM_SUFFIXES) {
    // 名字以「_后缀」结尾：oak_wall_sign -> oak_sign
    if (path.endsWith(`_${suffix}`)) {
      const base = suffix.replace("wall_", "");
      return ns + path.slice(0, path.length - suffix.length) + base;
    }
  }
  if (CAULDRON_VARIANTS.includes(path)) {
    return `${ns}cauldron`;
  }
  if (ANVIL_VARIANTS.includes(path)) {
    return `${ns}anvil`;
  }
  return blockName;
}
