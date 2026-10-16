import { describe, expect, it } from "vitest";
import { itemsForState } from "../utils/material_items";

/**
 * 口径照投影模组 Litematica 的 MaterialCache（材料清单）：
 * 状态换算成「建造它所需的物品与个数」。用例里的状态串取自真实投影。
 */
describe("方块状态折算成所需物品", () => {
  it("普通方块就是它自己，摆法差异归到同一个物品", () => {
    expect(itemsForState("minecraft:stone")).toEqual([
      { name: "minecraft:stone", count: 1 },
    ]);
    // 墙上的火把备的是火把
    expect(itemsForState("minecraft:wall_torch[facing=north]")).toEqual([
      { name: "minecraft:torch", count: 1 },
    ]);
    // 红石粉（线路）备的是红石粉
    expect(itemsForState("minecraft:redstone_wire[power=15]")).toEqual([
      { name: "minecraft:redstone_wire", count: 1 },
    ]);
  });

  it("不需要备料的状态不计数", () => {
    // 活塞头、移动中的活塞、各种传送门由别的方块生成
    expect(itemsForState("minecraft:piston_head[facing=north,short=false]")).toEqual([]);
    expect(itemsForState("minecraft:moving_piston")).toEqual([]);
    expect(itemsForState("minecraft:nether_portal[axis=x]")).toEqual([]);
    expect(itemsForState("minecraft:end_portal")).toEqual([]);
    expect(itemsForState("minecraft:end_gateway")).toEqual([]);
  });

  it("水与岩浆只算源头，折成一个桶", () => {
    expect(itemsForState("minecraft:water[level=0]")).toEqual([
      { name: "minecraft:water_bucket", count: 1 },
    ]);
    // 流动的水不需要备料
    expect(itemsForState("minecraft:water[level=3]")).toEqual([]);
    expect(itemsForState("minecraft:lava[level=0]")).toEqual([
      { name: "minecraft:lava_bucket", count: 1 },
    ]);
    expect(itemsForState("minecraft:lava[level=5]")).toEqual([]);
  });

  it("双格方块只算一次", () => {
    // 门的上半由下半一起放下
    expect(itemsForState("minecraft:oak_door[facing=north,half=lower]")).toEqual([
      { name: "minecraft:oak_door", count: 1 },
    ]);
    expect(itemsForState("minecraft:oak_door[facing=north,half=upper]")).toEqual([]);
    // 床的枕头同理
    expect(itemsForState("minecraft:white_bed[part=foot]")).toEqual([
      { name: "minecraft:white_bed", count: 1 },
    ]);
    expect(itemsForState("minecraft:white_bed[part=head]")).toEqual([]);
    // 高草的上半同理
    expect(itemsForState("minecraft:tall_grass[half=lower]")).toEqual([
      { name: "minecraft:tall_grass", count: 1 },
    ]);
    expect(itemsForState("minecraft:tall_grass[half=upper]")).toEqual([]);
  });

  it("一个状态需要多件的按状态折算数量", () => {
    // 双层台阶是两个台阶
    expect(itemsForState("minecraft:oak_slab[type=double]")).toEqual([
      { name: "minecraft:oak_slab", count: 2 },
    ]);
    expect(itemsForState("minecraft:oak_slab[type=bottom]")).toEqual([
      { name: "minecraft:oak_slab", count: 1 },
    ]);
    // 雪层、海龟蛋、海泡菜、蜡烛按状态里的数量
    expect(itemsForState("minecraft:snow[layers=5]")).toEqual([
      { name: "minecraft:snow", count: 5 },
    ]);
    expect(itemsForState("minecraft:turtle_egg[eggs=3]")).toEqual([
      { name: "minecraft:turtle_egg", count: 3 },
    ]);
    expect(itemsForState("minecraft:sea_pickle[pickles=2]")).toEqual([
      { name: "minecraft:sea_pickle", count: 2 },
    ]);
    expect(itemsForState("minecraft:candle[candles=4]")).toEqual([
      { name: "minecraft:candle", count: 4 },
    ]);
    // 发光地衣按附着面数
    expect(
      itemsForState(
        "minecraft:glow_lichen[down=true,up=false,north=true,south=false,east=true,west=false]",
      ),
    ).toEqual([{ name: "minecraft:glow_lichen", count: 3 }]);
  });

  it("带花的花盆要花盆与植物各一件", () => {
    expect(itemsForState("minecraft:potted_oak_sapling")).toEqual([
      { name: "minecraft:flower_pot", count: 1 },
      { name: "minecraft:oak_sapling", count: 1 },
    ]);
  });

  it("耕地备的是泥土", () => {
    expect(itemsForState("minecraft:farmland[moisture=7]")).toEqual([
      { name: "minecraft:dirt", count: 1 },
    ]);
  });

  it("没有属性的状态串也能处理", () => {
    expect(itemsForState("minecraft:chest")).toEqual([
      { name: "minecraft:chest", count: 1 },
    ]);
    expect(itemsForState("water[level=0]")).toEqual([
      { name: "water_bucket", count: 1 },
    ]);
  });
});
