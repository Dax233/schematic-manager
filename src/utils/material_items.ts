/**
 * 方块状态 → 建造它所需的物品与个数。
 *
 * 统计口径照投影模组 Litematica 的 `MaterialCache`（材料清单的核心）：
 * 先把方块按状态计数，再换算成「造出这个状态需要什么物品、要几个」，最后按
 * 物品累加。这样同一个物品的不同状态与不同摆法自然合成一行，而且数量是真正
 * 要备的物品数，而不是方块格数。
 *
 * 具体规则（与 MaterialCache 一一对应）：
 *
 *   1. **不需要物品**的状态不计入：活塞头、移动中的活塞、下界传送门、
 *      末地传送门、末地折跃门——它们由别的方块生成，玩家不用备料。
 *   2. **水与岩浆只有源头**（`level=0`）算一个桶，流动的部分不算：
 *      备料时只需要能倒出这些源头的水桶/岩浆桶。
 *   3. **双格方块只算一次**：门的上半、床的枕头、高草的上半不重复计，
 *      它们在放下下半时一起出现。
 *   4. **一个状态需要多个物品**：带花的花盆要「花盆 + 植物」两件。
 *   5. **数量按状态折算**：双层台阶算 2 个，雪层/海龟蛋/海泡菜/蜡烛按
 *      状态里的数量算，发光地衣与幽匿脉络按附着面数算。
 *   6. 其余方块所需物品就是它自己，摆法差异（墙上的火把/立地告示牌等）
 *      由 itemKeyOf 归到同一个物品上。
 *
 * 与统计表的关系：表里的每一行是一个**物品**，数量是所需物品数。因此
 * 「数量」不总等于方块格数（台阶会更多、门会更少、流动的水不出现）。
 */
import { itemKeyOf } from "./block_item";

/** 由别的方块生成、不需要玩家备料的状态 */
const NO_ITEM_BLOCKS = [
  "piston_head",
  "moving_piston",
  "nether_portal",
  "end_portal",
  "end_gateway",
];

/** 双格方块的上半/后半：放下半时一起出现，不重复计 */
const TALL_PLANTS = [
  "sunflower",
  "lilac",
  "rose_bush",
  "peony",
  "tall_grass",
  "large_fern",
];

/** 按附着面数计数的方块（MultifaceGrowthBlock） */
const MULTIFACE_BLOCKS = ["glow_lichen", "sculk_vein"];

/** 需要按状态里的数字折算数量的属性名 */
const COUNT_PROPS = ["layers", "eggs", "pickles", "candles"];

/** 一个物品与它在该状态下的个数 */
export interface StateItem {
  name: string;
  count: number;
}

/** 把状态串拆成方块名与属性表 */
function splitState(state: string): { name: string; props: Map<string, string> } {
  const i = state.indexOf("[");
  if (i < 0) return { name: state.trim(), props: new Map() };
  const name = state.slice(0, i).trim();
  const props = new Map<string, string>();
  for (const part of state.slice(i + 1).replace(/\]\s*$/, "").split(",")) {
    const eq = part.indexOf("=");
    if (eq > 0) props.set(part.slice(0, eq).trim(), part.slice(eq + 1).trim());
  }
  return { name, props };
}

/** 去掉命名空间，便于按方块类别判断 */
function shortName(name: string): string {
  const i = name.indexOf(":");
  return i >= 0 ? name.slice(i + 1) : name;
}

/**
 * 建造一个方块状态需要哪些物品、各几个。
 *
 * 返回空数组表示这个状态不需要备料（活塞头、流动的水、门的上半等），
 * 统计时应跳过。
 */
export function itemsForState(state: string): StateItem[] {
  const { name, props } = splitState(state);
  const short = shortName(name);
  const prefix = name.slice(0, name.length - short.length);

  if (NO_ITEM_BLOCKS.includes(short)) return [];
  // 耕地是锄出来的，备料备的是泥土
  if (short === "farmland") return [{ name: `${prefix}dirt`, count: 1 }];
  // 水与岩浆：只有源头需要一个桶
  if (short === "water" || short === "lava") {
    if (props.get("level") !== "0") return [];
    return [{ name: `${prefix}${short}_bucket`, count: 1 }];
  }
  // 双格方块的上半：由下半一起放下
  if (short.endsWith("_door") && props.get("half") === "upper") return [];
  if (short.endsWith("_bed") && props.get("part") === "head") return [];
  if (TALL_PLANTS.includes(short) && props.get("half") === "upper") return [];

  // 带花的花盆：花盆与植物各算一件
  if (short.startsWith("potted_")) {
    return [
      { name: `${prefix}flower_pot`, count: 1 },
      { name: itemKeyOf(`${prefix}${short.slice("potted_".length)}`), count: 1 },
    ];
  }

  const item = itemKeyOf(name);
  if (props.get("type") === "double") {
    // 双层台阶是两个台阶叠起来的
    return [{ name: item, count: 2 }];
  }
  for (const key of COUNT_PROPS) {
    const v = props.get(key);
    if (v !== undefined) {
      const n = Number(v);
      if (Number.isFinite(n) && n > 0) return [{ name: item, count: n }];
    }
  }
  if (MULTIFACE_BLOCKS.includes(short)) {
    // 每个附着面要用一个
    let faces = 0;
    for (const dir of ["down", "up", "north", "south", "east", "west"]) {
      if (props.get(dir) === "true") faces += 1;
    }
    if (faces > 0) return [{ name: item, count: faces }];
  }
  return [{ name: item, count: 1 }];
}
