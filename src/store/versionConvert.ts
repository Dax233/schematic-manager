/**
 * 版本转换域：转换弹窗的状态与流程（目标版本选择、差异分析、执行转换）。
 *
 * 与方块替换一样是向导式弹窗流程，状态自包含；拆开只是为了让
 * 「改方块」和「升版本」两条互不相关的链路各自清晰。
 *
 * 与其他域的协作：转换生成新文件后，经 VersionConvertDeps 让文件列表域
 * 重扫并收敛选中与勾选。依赖只声明本域用到的成员，避免类型上的
 * 循环引用（见 store/fileList.ts）。
 */
import { computed, ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  analyzeConversion,
  applyConversion,
  convertTargets,
  searchBlocks,
} from "../api";
import type {
  BlockHit,
  ConvertItem,
  ConvertTarget,
  SchematicSummary,
} from "../types";

/** 版本转换域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface VersionConvertDeps {
  /** 文件列表域的当前选中；转换的作用对象 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的文件清单；转换后收敛选中与勾选 */
  files: ComputedRef<SchematicSummary[]>;
  /** 文件列表域的重扫；转换生成新文件后刷新清单 */
  rescanAll: () => Promise<void>;
  /** 文件列表域的选中收敛；清单变化后保持选中有效 */
  syncSelection: (items: SchematicSummary[]) => void;
  /** 文件列表域的勾选清理；清单变化后去掉已不存在的勾选 */
  pruneChecks: (items: SchematicSummary[]) => void;
}

export function createVersionConvertSlice(s: VersionConvertDeps) {
  // 版本转换
  const convOpen = ref(false);
  const convBusy = ref(false);
  const convTargets = ref<ConvertTarget[]>([]);
  const convTargetIdx = ref<number | undefined>(undefined);
  const convItems = ref<ConvertItem[]>([]);
  const convAnalyzed = ref(false);

  /** 正在被指定替代品的源方块 ID。
   *
   * 右侧方块列表点选后填到这一行。表格与列表是两个区域，必须让用户看出
   * 「现在点方块会填到哪一行」，否则点下去不知道改的是谁。
   */
  const convActiveName = ref("");

  // 右侧方块列表：搜索并点选替代品
  const convPickQuery = ref("");
  const convPickHits = ref<BlockHit[]>([]);
  const convPickBusy = ref(false);
  /** 搜索请求序号：逐字符搜索会并发多个请求，只用最新那个的结果 */
  let convPickSeq = 0;

  /** 方块 ID -> 中文名。查过但方块库里没有时记为空串 */
  const convZh = ref<Record<string, string>>({});
  /** 已经查过的方块 ID。
   *
   * 与 convZh 分开是为了区分「还没查」与「库里没有这个名字」：
   * 只看有没有中文名的话，两种情况的界面表现会一样，用户分不清
   * 是自己还没查还是这个方块本来就没有中文名。
   */
  const convZhChecked = ref<Set<string>>(new Set());
  /** 手输方块 ID 后查中文名的去抖计时器，按方块名分开存。
   *
   * 不能共用一个计时器：那样在两行之间快速输入时，后一次输入会把前一行
   * 还没落地的查询取消掉，那一行的中文名就再也不显示了。 */
  const convZhTimers = new Map<string, ReturnType<typeof setTimeout>>();

  const convColumns = [
    { title: "原方块（目标版本里没有）", slotName: "from" },
    { title: "数量", dataIndex: "count", width: 80 },
    { title: "替代为", slotName: "repl" },
    { title: "状态", slotName: "state", width: 96 },
  ];

  /** 这一项最终会用到的替代品：已指定优先，其次自动推荐 */
  function convReplacementOf(i: ConvertItem): string {
    return i.chosen.trim() || i.suggested;
  }

  /** 这一项还没有替代品：既没有自动推荐，用户也没指定 */
  function convNeedsPick(i: ConvertItem): boolean {
    return !i.chosen.trim() && !i.suggested;
  }

  /** 这一项的替代品是自动识别出来的（有推荐且用户没有另选） */
  function convIsAuto(i: ConvertItem): boolean {
    return !i.chosen.trim() && Boolean(i.suggested);
  }

  /** 「替代为」那一格的状态，决定输入框的颜色。
   *
   * 三种状态各自对应一种输入框外观：
   *   todo 还没定替代品——红框，要用户处理；
   *   auto 自动识别出来的推荐——绿框，提示「照这个转就行」；
   *   set  用户自己选过或填过——不上色，颜色只在需要提示时才出现，
   *        用户已经做过决定的行不必再抢注意力。
   */
  function convReplState(i: ConvertItem): "todo" | "auto" | "set" {
    if (convNeedsPick(i)) return "todo";
    return convIsAuto(i) ? "auto" : "set";
  }

  /** 方块中文名；没有记录时返回空串 */
  function convZhOf(id: string): string {
    return id ? convZh.value[id] ?? "" : "";
  }

  /** 这个方块是否已经查过名字（用于区分「没查」与「库里没有」） */
  function convZhQueried(id: string): boolean {
    return convZhChecked.value.has(id);
  }

  function rememberZh(id: string, zh: string) {
    const next = { ...convZh.value, [id]: zh };
    const checked = new Set(convZhChecked.value);
    checked.add(id);
    convZh.value = next;
    convZhChecked.value = checked;
  }

  /** 点左侧某一行：右侧列表接下来填的就是它 */
  function selectConvRow(item: ConvertItem) {
    convActiveName.value = item.name;
  }

  /** 当前正在指定替代品的那一项；没有（还没点过任何行）时为 null */
  const convActiveItem = computed(
    () => convItems.value.find((i) => i.name === convActiveName.value) ?? null,
  );

  /** 这个方块是否存在于目标版本。
   *
   * 检索是跨**全部版本**做的，而替代品必须是目标版本里有的方块：否则转换
   * 出来的文件仍然引用一个不存在的方块，正是这个窗口要消除的问题。
   * 方块库完全不认识的 ID（模组方块）无从判断，这里按「不在目标版本」处理，
   * 由调用方决定是拦下还是只提醒。
   */
  function convHitInTarget(hit: BlockHit): boolean {
    const idx = convTargetIdx.value;
    if (idx === undefined) return false;
    return hit.versions.some((v) => v.index === idx);
  }

  /** 在右侧列表里搜索方块 */
  async function onConvPickSearch(text: string) {
    const q = text.trim();
    // 记下这次查询的序号：逐字符搜索会并发多个请求，慢的旧响应回来时
    // 不能覆盖新结果，否则列表内容与输入框里的关键字对不上
    const seq = ++convPickSeq;
    if (!q) {
      convPickHits.value = [];
      // 空查询自己就是最新的一次查询，而它在途的前一次查询已经因为序号
      // 不匹配不再复位这个标志；这里必须复位，否则「输入后立刻清空」会把
      // 转圈状态永久留在界面上
      convPickBusy.value = false;
      return;
    }
    convPickBusy.value = true;
    try {
      const hits = await searchBlocks(q, 200);
      if (seq !== convPickSeq) return; // 已有更新的查询，丢弃这份过期结果
      // 目标版本里有的方块排前面：那些才是能用的替代品，翻起来不必先越过
      // 一堆点不了的项。sort 在现代 JS 里是稳定的，所以两组内部仍保持
      // 后端给出的相关度顺序。
      convPickHits.value = [...hits].sort(
        (a, b) => Number(convHitInTarget(b)) - Number(convHitInTarget(a)),
      );
      // 顺手把结果里的中文名记下来（并标记为已查过），点选时就不必再查
      const zh = { ...convZh.value };
      const checked = new Set(convZhChecked.value);
      for (const h of hits) {
        zh[h.name] = h.zh || h.en || "";
        checked.add(h.name);
      }
      convZh.value = zh;
      convZhChecked.value = checked;
    } catch (e) {
      if (seq !== convPickSeq) return;
      Message.error(`检索方块失败：${e}`);
    } finally {
      if (seq === convPickSeq) convPickBusy.value = false;
    }
  }

  /** 点选右侧列表里的方块，填给当前选中的那一行 */
  function pickConvBlock(hit: BlockHit) {
    const target = convItems.value.find((i) => i.name === convActiveName.value);
    if (!target) {
      Message.warning("请先在左侧点选要替换的方块，再点这里添加");
      return;
    }
    if (!convHitInTarget(hit)) {
      Message.warning(
        `目标版本里没有「${hit.zh || hit.name}」，不能作为替代品`,
      );
      return;
    }
    target.chosen = hit.name;
    rememberZh(hit.name, hit.zh || hit.en || "");
    Message.success(
      `已把「${hit.zh || hit.name}」指定为 ${target.nameZh || target.name} 的替代品`,
    );
  }

  /** 手输的方块 ID 也要显示中文名：改动后延迟查一次（逐字符查太费） */
  function scheduleConvZhLookup(item: ConvertItem) {
    const key = item.name;
    const pending = convZhTimers.get(key);
    if (pending !== undefined) clearTimeout(pending);
    convZhTimers.set(
      key,
      setTimeout(() => {
        convZhTimers.delete(key);
        void lookupConvZh(item.chosen.trim());
      }, 300),
    );
  }

  /** 查一个方块 ID 的中文名。查不到时记为空串（界面据此说明库里没有） */
  async function lookupConvZh(id: string) {
    if (!id || convZhQueried(id)) return;
    try {
      const hits = await searchBlocks(id, 50);
      const exact = hits.find((h) => h.name.toLowerCase() === id.toLowerCase());
      rememberZh(id, exact ? exact.zh || exact.en || "" : "");
      // 方块库认识这个 ID、但它不在目标版本里：如实提醒一句。
      // 库里完全不认识（模组方块）时不作判断——那种情况下没依据，也不该拦。
      if (exact && !convHitInTarget(exact)) {
        Message.warning(
          `目标版本里没有「${exact.zh || exact.name}」，转换后仍会指向它`,
        );
      }
    } catch (e) {
      // 查不到名字不该拦住填写，但也不能悄悄当成「库里没有」：
      // 记为未查过，下次改动会再试一遍
      Message.error(`查询方块中文名失败：${e}`);
    }
  }

  /** 重新读取可转换版本。
   *
   * 单独成函数是因为「版本转换」窗口里点一个尚未加载的版本会现场补库，
   * 补完必须重取一次列表——新版本在方块库里的下标只有后端知道。
   */
  async function reloadConvTargets(): Promise<void> {
    if (!s.selected.value) {
      convTargets.value = [];
      return;
    }
    try {
      convTargets.value = await convertTargets(s.selected.value.path);
    } catch (e) {
      Message.error(`读取可转换版本失败：${e}`);
      convTargets.value = [];
    }
  }

  async function openConvert() {
    convOpen.value = true;
    convItems.value = [];
    convAnalyzed.value = false;
    convTargetIdx.value = undefined;
    // 上一次的搜索结果与中文名缓存都属于上一个投影，不能带过来
    convPickQuery.value = "";
    convPickHits.value = [];
    convActiveName.value = "";
    convZh.value = {};
    convZhChecked.value = new Set();
    // 上一次没落地的中文名查询也要取消：否则它会在新会话里写进刚清空的缓存
    for (const t of convZhTimers.values()) clearTimeout(t);
    convZhTimers.clear();
    if (!s.selected.value) return;
    await reloadConvTargets();
  }

  async function runConvertAnalyze() {
    const cur = s.selected.value;
    if (!cur || convTargetIdx.value === undefined) {
      Message.warning("请先选择目标版本");
      return;
    }
    convBusy.value = true;
    try {
      convItems.value = await analyzeConversion(cur.path, convTargetIdx.value);
      convAnalyzed.value = true;
      // 分析结果自带源方块与自动推荐的中文名，一次收进缓存：
      // 界面上凡出现方块 ID 的地方都要显示中文名，逐条回查既慢又没必要
      const zh: Record<string, string> = { ...convZh.value };
      const checked = new Set(convZhChecked.value);
      for (const i of convItems.value) {
        zh[i.name] = i.nameZh || "";
        checked.add(i.name);
        if (i.suggested) {
          zh[i.suggested] = i.suggestedZh || "";
          checked.add(i.suggested);
        }
      }
      convZh.value = zh;
      convZhChecked.value = checked;
      // 刻意不预选第一行：预选之后用户在右侧随手点一个方块就会改掉第一行，
      // 而那一行未必是他想改的。让他先明确点一行，右侧提示也才有的放矢。
      convActiveName.value = "";
      const missing = convItems.value.filter(convNeedsPick).length;
      if (convItems.value.length === 0) {
        Message.success("该投影用到的方块在目标版本中都存在，可以直接转换");
      } else {
        Message.info(`目标版本缺少 ${convItems.value.length} 种方块，其中 ${missing} 种没有自动推荐`);
      }
    } catch (e) {
      Message.error(`分析失败：${e}`);
      convItems.value = [];
      // 必须复位分析标志。否则模板里「convAnalyzed 且条目为空」这一组合
      // 会被判成「方块都存在、可以直接转换」，向用户展示绿色的成功提示，
      // 而实际是分析失败、根本没有结果。
      convAnalyzed.value = false;
    } finally {
      convBusy.value = false;
    }
  }

  async function doConvert() {
    const cur = s.selected.value;
    if (!cur || convTargetIdx.value === undefined) return;
    convBusy.value = true;
    try {
      const r = await applyConversion(cur.path, convTargetIdx.value, convItems.value);
      Message.success(
        `已转换：替换 ${r.replacedKinds} 种（${r.replacedBlocks} 个）方块` +
          (r.skippedKinds > 0 ? `，${r.skippedKinds} 种未指定已保持原样` : "") +
          `。新文件：${r.outputPath}`,
      );
      convOpen.value = false;
      // 转换会生成新文件并可能改动原文件；重扫全部已加载目录后再定位选中项
      await s.rescanAll();
      s.syncSelection(s.files.value);
      s.pruneChecks(s.files.value);
    } catch (e) {
      Message.error(`转换失败：${e}`);
    } finally {
      convBusy.value = false;
    }
  }

  return {
    convOpen,
    convBusy,
    convTargets,
    convTargetIdx,
    convItems,
    convAnalyzed,
    convColumns,
    convActiveItem,
    convPickQuery,
    convPickHits,
    convPickBusy,
    convReplacementOf,
    convNeedsPick,
    convIsAuto,
    convReplState,
    convHitInTarget,
    convZhOf,
    convZhQueried,
    selectConvRow,
    onConvPickSearch,
    pickConvBlock,
    scheduleConvZhLookup,
    openConvert,
    reloadConvTargets,
    runConvertAnalyze,
    doConvert,
  };
}

export type VersionConvertSlice = ReturnType<typeof createVersionConvertSlice>;
