import { describe, expect, it } from "vitest";
import { itemKeyOf } from "../utils/block_item";

/** 用例里的方块名取自真实投影中出现过的名字 */
describe("方块名归并成物品键", () => {
  it("墙挂形态与落地形态归到同一个物品", () => {
    // 火把：地上的火把与墙上的火把是同一个物品
    expect(itemKeyOf("minecraft:wall_torch")).toBe("minecraft:torch");
    expect(itemKeyOf("minecraft:torch")).toBe("minecraft:torch");
    // 红石火把同理
    expect(itemKeyOf("minecraft:redstone_wall_torch")).toBe("minecraft:redstone_torch");
    expect(itemKeyOf("minecraft:redstone_torch")).toBe("minecraft:redstone_torch");
    // 告示牌：木种前缀各不相同，去掉墙挂中缀后与立地告示牌同名
    expect(itemKeyOf("minecraft:oak_wall_sign")).toBe("minecraft:oak_sign");
    expect(itemKeyOf("minecraft:bamboo_wall_sign")).toBe("minecraft:bamboo_sign");
    expect(itemKeyOf("minecraft:birch_wall_sign")).toBe("minecraft:birch_sign");
    expect(itemKeyOf("minecraft:cherry_wall_sign")).toBe("minecraft:cherry_sign");
    expect(itemKeyOf("minecraft:spruce_wall_sign")).toBe("minecraft:spruce_sign");
    // 只有墙挂形态出现在投影里时也要归并（同一次统计里见到墙挂就归到物品名上）
    expect(itemKeyOf("minecraft:pale_oak_wall_sign")).toBe("minecraft:pale_oak_sign");
    // 悬挂告示牌、旗帜、头、珊瑚扇属于同一规律
    expect(itemKeyOf("minecraft:oak_wall_hanging_sign")).toBe(
      "minecraft:oak_hanging_sign",
    );
    expect(itemKeyOf("minecraft:red_wall_banner")).toBe("minecraft:red_banner");
    expect(itemKeyOf("minecraft:skeleton_wall_skull")).toBe("minecraft:skeleton_skull");
    expect(itemKeyOf("minecraft:player_wall_head")).toBe("minecraft:player_head");
    expect(itemKeyOf("minecraft:dead_tube_coral_wall_fan")).toBe(
      "minecraft:dead_tube_coral_fan",
    );
  });

  it("炼药锅的内容物与铁砧的破损程度归到同一个物品", () => {
    // 游戏里只有一个「炼药锅」物品，装什么进去都是它
    expect(itemKeyOf("minecraft:lava_cauldron")).toBe("minecraft:cauldron");
    expect(itemKeyOf("minecraft:water_cauldron")).toBe("minecraft:cauldron");
    expect(itemKeyOf("minecraft:powder_snow_cauldron")).toBe("minecraft:cauldron");
    expect(itemKeyOf("minecraft:cauldron")).toBe("minecraft:cauldron");
    // 游戏里只有一个「铁砧」物品，裂痕是使用痕迹
    expect(itemKeyOf("minecraft:chipped_anvil")).toBe("minecraft:anvil");
    expect(itemKeyOf("minecraft:damaged_anvil")).toBe("minecraft:anvil");
    expect(itemKeyOf("minecraft:anvil")).toBe("minecraft:anvil");
  });

  it("不能误合并：名字相似但确是两种物品的方块保持原样", () => {
    // 真正的墙方块：以 _wall 结尾，不含墙挂中缀
    expect(itemKeyOf("minecraft:cobblestone_wall")).toBe("minecraft:cobblestone_wall");
    expect(itemKeyOf("minecraft:stone_brick_wall")).toBe("minecraft:stone_brick_wall");
    expect(itemKeyOf("minecraft:prismarine_wall")).toBe("minecraft:prismarine_wall");
    // 涂蜡前后是两个物品
    expect(itemKeyOf("minecraft:waxed_lightning_rod")).toBe("minecraft:waxed_lightning_rod");
    expect(itemKeyOf("minecraft:lightning_rod")).toBe("minecraft:lightning_rod");
    // 氧化程度不同也是不同物品
    expect(itemKeyOf("minecraft:exposed_copper_chain")).toBe("minecraft:exposed_copper_chain");
    expect(itemKeyOf("minecraft:copper_chain")).toBe("minecraft:copper_chain");
    // 普通方块原样返回
    expect(itemKeyOf("minecraft:stone")).toBe("minecraft:stone");
    expect(itemKeyOf("minecraft:oak_slab")).toBe("minecraft:oak_slab");
    expect(itemKeyOf("minecraft:chest")).toBe("minecraft:chest");
    expect(itemKeyOf("minecraft:water")).toBe("minecraft:water");
  });

  it("模组方块里 wall_ 本就属于名字时不当作墙挂形态", () => {
    // 按后缀匹配而不是子串替换，避免把模组自己的方块并到另一个名字上
    expect(itemKeyOf("mod:wall_panel")).toBe("mod:wall_panel");
    expect(itemKeyOf("mod:wall_sign")).toBe("mod:wall_sign");
    expect(itemKeyOf("mod:my_wall_thing")).toBe("mod:my_wall_thing");
  });

  it("不带命名空间的名字也能处理，且不跨命名空间归并", () => {
    // 真实投影里出现过不带命名空间的 id，归并规律必须同样成立
    expect(itemKeyOf("wall_torch")).toBe("torch");
    expect(itemKeyOf("oak_wall_sign")).toBe("oak_sign");
    expect(itemKeyOf("lava_cauldron")).toBe("cauldron");
    expect(itemKeyOf("stone")).toBe("stone");
    // 命名空间不同的同名方块不能被并到一起
    expect(itemKeyOf("minecraft:oak_wall_sign")).not.toBe(itemKeyOf("other:oak_wall_sign"));
  });
});
