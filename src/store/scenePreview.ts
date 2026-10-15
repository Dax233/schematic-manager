/**
 * 3D 预览与方块统计域：场景加载、视角/亮度/导出、方块渲染图缓存、
 * 方块统计表的列宽与文本导出。
 *
 * 这些状态都服务于右侧详情区：选中一个投影后，场景、统计、渲染图
 * 一起出现、一起失效（换投影时 resetBlockIcons 清空），放在一起
 * 才能看清这条「选中 → 加载 → 展示」链路的完整生命周期。
 *
 * 与其他域的协作：经 ScenePreviewDeps 读文件列表域的 selected/analysis，
 * 批量导出读取文件列表域的勾选项。依赖只声明本域用到的成员，
 * 而不是整棵 store，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { computed, ref, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { TableColumnData } from "@arco-design/web-vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import { buildScene, renderBlockIcons } from "../api";
import type SceneViewer from "../components/SceneViewer.vue";
import { OffscreenRenderer } from "../render/scene3d";
import { NamePool, joinPath, previewStem } from "../render/export_name";
import type { AnalysisResult, BlockRow, SceneData, SchematicSummary } from "../types";
import { itemKeyOf } from "../utils/block_item";
import { formatStacks } from "../utils/format";

/** 3D 预览与方块统计域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface ScenePreviewDeps {
  /** 文件列表域的当前选中；加载场景、导出命名都围绕它 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的方块分析结果；统计表与图标预取的数据源 */
  analysis: Ref<AnalysisResult | null>;
  /** 文件列表域已勾选且无解析错误的文件；批量导出的对象 */
  checkedFiles: ComputedRef<SchematicSummary[]>;
}

export function createScenePreviewSlice(s: ScenePreviewDeps) {
  const viewer = ref<InstanceType<typeof SceneViewer> | null>(null);
  const scene = ref<SceneData | null>(null);
  const sceneLoading = ref(false);
  const sceneError = ref("");
  const brightness = ref(1.45);
  const activeView = ref("left45");
  const exportSize = ref(1024);
  const exporting = ref(false);
  /** 批量导出的进度（null 表示未在批量导出） */
  const exportingProgress = ref<{ done: number; total: number } | null>(null);

  // 场景加载失败不弹消息：错误长期展示在预览区里（sceneError），
  // 与「点一下、弹一条就消失」的瞬时操作不同
  async function loadScene(record: SchematicSummary) {
    scene.value = null;
    sceneError.value = "";
    sceneLoading.value = true;
    try {
      scene.value = await buildScene(record.path);
    } catch (e) {
      sceneError.value = String(e);
    } finally {
      sceneLoading.value = false;
    }
  }

  const sceneInfo = computed(() => {
    if (!scene.value) return "";
    const cur = scene.value;
    const parts = [`${cur.totalTriangles.toLocaleString()} 个三角形`];
    if (cur.unresolvedBlocks > 0) parts.push(`${cur.unresolvedBlocks} 种方块无贴图`);
    return parts.join("　");
  });

  function setView(name: string) {
    activeView.value = name;
    viewer.value?.applyPreset(name);
  }

  /** 导出当前视角为透明背景 PNG（单个文件） */
  async function exportPreview() {
    const cur = s.selected.value;
    if (!cur || !scene.value) return;
    const size = exportSize.value;
    const stem = cur.fileName.replace(/\.litematic$/i, "");
    // 同 pickDirectory：保存对话框失败时必须给出提示，
    // 放在 try 外会让这个 Promise 静默 reject，用户点了导出却没反应
    let target: unknown;
    try {
      target = await save({
        title: "导出预览图",
        defaultPath: `${stem}_preview_${size}.png`,
        filters: [{ name: "PNG 图片", extensions: ["png"] }],
      });
    } catch (e) {
      Message.error(`打开保存对话框失败：${e}`);
      return;
    }
    if (typeof target !== "string") return;

    exporting.value = true;
    try {
      const bytes = await viewer.value?.exportPng(size);
      if (!bytes) throw new Error("3D 视图尚未就绪");
      await writeFile(target, bytes);
      Message.success(`已导出 ${size}×${size} 预览图`);
    } catch (e) {
      Message.error(`导出失败：${e}`);
    } finally {
      exporting.value = false;
    }
  }

  /** 选择批量导出的目标目录 */
  async function pickExportDir(): Promise<string | null> {
    try {
      const dir = await open({ directory: true, multiple: false, title: "选择导出目录" });
      return typeof dir === "string" ? dir : null;
    } catch (e) {
      // 同其它对话框：失败必须提示，否则用户点了没反应
      Message.error(`打开目录选择框失败：${e}`);
      return null;
    }
  }

  /**
   * 批量导出预览图。
   *
   * 用独立的离屏渲染器逐个渲染，而不是把界面切到每个文件再导出：
   * 后者会反复冲掉用户正在看的画面，还要等每次界面重建，明显更慢。
   *
   * 单个文件失败不中断整批——某个投影读不出来（被占用、格式异常）时
   * 其余文件仍应导出成功，最后把失败清单如实报出来。
   */
  async function exportBatch() {
    const list = s.checkedFiles.value.filter((f) => !f.error);
    if (list.length === 0) {
      Message.warning("请先勾选要导出的投影文件");
      return;
    }
    const dir = await pickExportDir();
    if (!dir) return;

    const size = exportSize.value;
    const view = activeView.value;
    exporting.value = true;
    exportingProgress.value = { done: 0, total: list.length };
    let renderer: OffscreenRenderer | null = null;
    const failures: string[] = [];
    let ok = 0;
    // 不同目录下可能有同名投影，直接按文件名写会互相覆盖——
    // 导出 10 张最后只剩 1 张，而且没有任何提示。因此用带序号的命名池。
    const names = new NamePool();

    try {
      renderer = new OffscreenRenderer();
      for (const f of list) {
        try {
          const data = await buildScene(f.path);
          const bytes = await renderer.renderPng(data, size, view);
          const name = names.take(previewStem(f.fileName, size), ".png");
          await writeFile(joinPath(dir, name), bytes);
          ok += 1;
        } catch (e) {
          failures.push(`${f.fileName}：${e}`);
        }
        exportingProgress.value = { done: ok + failures.length, total: list.length };
      }
    } catch (e) {
      // 离屏渲染器都建不起来（例如 WebGL 上下文耗尽），整批无法进行
      Message.error(`批量导出失败：${e}`);
      return;
    } finally {
      renderer?.dispose();
      exporting.value = false;
      exportingProgress.value = null;
    }

    if (failures.length === 0) {
      Message.success(`已导出 ${ok} 张 ${size}×${size} 预览图到 ${dir}`);
    } else {
      Message.warning({
        content: `成功 ${ok} 张，失败 ${failures.length} 张：\n${failures.join("\n")}`,
        duration: 8000,
      });
    }
  }

  // ---------------------------------------------------------------- 方块统计与渲染图

  /** 方块栏列。
   *
   * 中文名在前、ID 在后：玩家认的是「橡树树叶」而不是「minecraft:oak_leaves」，
   * 但出问题时仍需要看到 ID，因此两者都保留。
   * 中文名查不到时由 slot 回退显示 ID（见模板里的 zh 插槽）。
   *
   * 最后一列是「堆叠」，需要可拖动就必须让**它之后**还有一列：
   * Arco 判定 resizable 要求 `index2 < row.length - 1`（table.js:1247），
   * 否则最后一列永远没有拖动手柄。这里同样补一个 1px 占位列。
   */
  const blockColumns: TableColumnData[] = [
    { title: "渲染图", slotName: "icon", width: 60 },
    { title: "中文名", slotName: "zhName", width: 150, ellipsis: true, tooltip: true },
    { title: "方块", dataIndex: "name", ellipsis: true, tooltip: true },
    // 状态走插槽：合并显示的行要写「含 N 种状态」而不是某一个代表状态
    { title: "状态", dataIndex: "state", slotName: "state", ellipsis: true, tooltip: true },
    { title: "数量", dataIndex: "count", width: 90 },
    { title: "堆叠", slotName: "stacks", width: 110 },
    {
      title: "",
      dataIndex: "__block_filler",
      width: 1,
      headerCellClass: "block__filler-col",
      bodyCellClass: "block__filler-col",
    },
  ];

  /** 方块栏列宽：由用户拖动表头调整，与侧栏同一套做法。
   *
   * 单独一份而不是共用侧栏的 colWidths：两张表的列完全不同，
   * 共用会让拖动侧栏把方块栏的宽度也改掉。
   */
  const blockColWidths = ref<Record<string, number>>({});
  /** 方块栏表格的 key：复位列宽靠重建（Arco 内部记了宽度，外部改不生效） */
  const blockTableKey = ref(0);

  function onBlockColumnResize(dataIndex: string, width: number) {
    if (!dataIndex || dataIndex === "__block_filler") return;
    blockColWidths.value = { ...blockColWidths.value, [dataIndex]: width };
  }

  /** 实际渲染用的方块栏列：把用户调整过的宽度套回去，并让最后一列可拖 */
  const blockTableColumns = computed<TableColumnData[]>(() =>
    blockColumns.map((c) => {
      const key = String(c.dataIndex ?? "");
      const w = key ? blockColWidths.value[key] : undefined;
      return w ? { ...c, width: w } : c;
    }),
  );

  function resetBlockColumnWidths() {
    blockColWidths.value = {};
    blockTableKey.value += 1;
  }

  /** 方块栏每页行数。渲染图按页加载，所以这个值同时决定了加载节奏。 */
  const BLOCK_PAGE_SIZE = 50;
  /** 一次请求多少张渲染图。远小于后端的单批上限，避免一次塞进过大的 IPC 负载。 */
  const ICON_BATCH = 128;
  const blockPage = ref(1);

  /** 方块统计表的行：按**物品**合计，忽略方块状态与摆放形态。
   *
   * 后端给的是「每个方块状态用了多少个」。要合计两层：
   *   1. 状态（红石粉的强度、铁轨的朝向与通电、楼梯的朝向……）；
   *   2. 同一个物品的不同方块形态（火把与墙上的火把、立地与墙上的告示牌、
   *      炼药锅的内容物、铁砧的破损程度）——见 utils/block_item.ts 的规律。
   * 用户要看的是「这种物品一共要备多少」，两层都属于摆放差异。
   *
   * 一行里如果出现了基础形态（例如同时有火把与墙上的火把），就用基础形态的
   * 名字、中文名与渲染图：那一行代表的是「火把」这个物品，而不是墙挂形态。
   * 跨了哪几个方块名记在 forms 里，替换时据此说明只覆盖了其中一个名字。
   */
  const blockRows = computed<BlockRow[]>(() => {
    const all = s.analysis.value?.blocks ?? [];
    const out: BlockRow[] = [];
    const at = new Map<string, number>();
    for (const b of all) {
      // 后端已按数量降序返回，因此同一方块第一项就是数量最多的那个状态
      const key = itemKeyOf(b.name);
      const i = at.get(key);
      if (i === undefined) {
        at.set(key, out.length);
        out.push({ ...b, stateCount: 1, forms: [b.name] });
        continue;
      }
      const row = out[i];
      row.count += b.count;
      row.stateCount += 1;
      if (!row.forms.includes(b.name)) row.forms.push(b.name);
      // 基础形态优先当代表：名字、中文名、渲染图都取它的
      if (b.name === key && row.name !== key) {
        row.name = b.name;
        row.zhName = b.zhName;
        row.enName = b.enName;
        row.state = b.state;
      }
    }
    // 合计后的数量与后端给出的单状态数量不同，名次要重排：否则「用了 8000 个
    // 的红石粉」会排在「用了 12 个的石头」后面。数量相同按名字排，结果稳定
    // （不用 localeCompare：它随环境语言数据变化，同一份数据在不同机器上会排出不同顺序）。
    out.sort((a, b) => b.count - a.count || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    return out;
  });

  /** 当前页的行。渲染图只给可见的这批，整表一次性加载会有几 MB 的 data URL。 */
  const blockPageRows = computed<BlockRow[]>(() => {
    const start = (blockPage.value - 1) * BLOCK_PAGE_SIZE;
    return blockRows.value.slice(start, start + BLOCK_PAGE_SIZE);
  });

  /** 状态一栏显示什么。
   *
   * 合计了多条记录的行不能只写它那一个代表状态：那一行的数量是所有这些记录的
   * 总和，写一个具体状态会让人以为「只有这么多是那个状态的」。跨了多个方块名
   * 时还要点明这一点——用户以为「这一行就是火把」，实际上墙上的火把也在里面。
   */
  function blockStateText(row: BlockRow): string {
    if (row.forms.length > 1) {
      return `（含 ${row.forms.length} 种形态、共 ${row.stateCount} 种状态）`;
    }
    return row.stateCount > 1 ? `（含 ${row.stateCount} 种状态）` : row.state;
  }

  /** 状态键 → data URL。用普通 Map 而不是 reactive：
   * 这里会有几百上千项，逐项做响应式代理得不偿失，
   * 改动后统一靠 iconVersion 触发重绘。 */
  const iconCache = new Map<string, string>();
  /** 状态键 → 渲染失败原因。同样需要缓存，否则每次翻页都会重试同一个坏方块。 */
  const iconErrors = new Map<string, string>();
  /** 渲染图缓存的版本号：Map 本身不是响应式的，靠它通知模板重新求值 */
  const iconVersion = ref(0);
  /** 换投影后旧请求的结果必须丢弃，否则会把上一个投影的图贴到新表上 */
  let iconGeneration = 0;

  /** 取某行的渲染图；没有时返回空串。 */
  function iconOf(state: string): string {
    void iconVersion.value; // 建立响应式依赖
    return iconCache.get(state) ?? "";
  }

  /** 取某行渲染图的失败原因；成功或尚未加载时返回空串。 */
  function iconErrorOf(state: string): string {
    void iconVersion.value;
    return iconErrors.get(state) ?? "";
  }

  /** 按需加载这一页里还没有渲染图的行。 */
  async function ensureBlockIcons(rows: BlockRow[]) {
    const dataVersion = s.selected.value?.dataVersion ?? 0;
    if (dataVersion <= 0) return;
    const missing = rows
      .map((r) => r.state)
      .filter((st) => !iconCache.has(st) && !iconErrors.has(st));
    if (missing.length === 0) return;

    const gen = iconGeneration;
    for (let i = 0; i < missing.length; i += ICON_BATCH) {
      const chunk = missing.slice(i, i + ICON_BATCH);
      let icons;
      try {
        icons = await renderBlockIcons(dataVersion, chunk);
      } catch (e) {
        if (gen !== iconGeneration) return;
        // 整批失败通常是方块库出了问题。把这一批标记成已处理，
        // 否则每次翻回这一页都会重试并再弹一次同样的错误。
        const reason = `渲染方块图失败：${e}`;
        for (const st of chunk) iconErrors.set(st, reason);
        iconVersion.value += 1;
        Message.error(reason);
        return;
      }
      if (gen !== iconGeneration) return;
      for (const icon of icons) {
        if (icon.error) iconErrors.set(icon.state, icon.error);
        else if (icon.dataUrl) iconCache.set(icon.state, icon.dataUrl);
      }
      iconVersion.value += 1;
    }
  }

  /** 换了投影就把渲染图与页码一起重置：缓存是按状态键存的，
   * 跨投影复用看起来没问题，但不同版本的同一状态可能贴图不同。
   *
   * 必须先把 generation 加一，正在飞的那批请求才会被丢弃；
   * 否则它们的回调会按老投影的图写进刚清空的缓存。 */
  function resetBlockIcons() {
    iconGeneration += 1;
    iconCache.clear();
    iconErrors.clear();
    iconVersion.value += 1;
    blockPage.value = 1;
  }

  watch(blockPageRows, (rows) => void ensureBlockIcons(rows), { immediate: true });

  /** 导出方块统计为文本表格（制表符分隔，可直接粘进 Excel）。
   *
   * 用制表符而不是逗号：方块名里含 `:` 与 `[`，但不会有制表符；
   * 名字里若真有制表符或换行会被替换掉，避免一行数据被拆成两行。
   * 行取合并后的 blockRows，与界面上看到的一致——导出与显示对不上
   * 会让人以为导出漏了方块。
   */
  function blockTableText(): string {
    const rows: string[] = [];
    rows.push(["中文名", "方块ID", "状态", "数量", "堆叠"].join("\t"));
    for (const b of blockRows.value) {
      const name = (b.zhName || b.enName || "").replace(/[\t\r\n]/g, " ");
      const state = blockStateText(b).replace(/[\t\r\n]/g, " ");
      rows.push([name, b.name, state, String(b.count), formatStacks(b.count)].join("\t"));
    }
    return rows.join("\r\n");
  }

  /** 把方块统计导出为 .txt 文件（用户可选保存位置） */
  async function exportBlockTable() {
    const blocks = blockRows.value;
    if (blocks.length === 0) {
      Message.warning("当前没有方块统计可导出");
      return;
    }
    const target = await save({
      defaultPath: `${s.selected.value?.fileName?.replace(/\.litematic$/i, "") ?? "方块统计"}-方块表.txt`,
      filters: [{ name: "文本文件", extensions: ["txt"] }],
    });
    if (!target) return; // 用户取消
    try {
      await writeFile(target, new TextEncoder().encode(blockTableText()));
      Message.success(`已导出 ${blocks.length} 行到 ${target}`);
    } catch (e) {
      Message.error(`导出失败：${e}`);
    }
  }

  /** 复制方块表到剪贴板 */
  async function copyBlockTable() {
    const blocks = blockRows.value;
    if (blocks.length === 0) {
      Message.warning("当前没有方块统计可复制");
      return;
    }
    try {
      await navigator.clipboard.writeText(blockTableText());
      Message.success(`已复制 ${blocks.length} 行`);
    } catch (e) {
      Message.error(`复制失败：${e}`);
    }
  }

  /** 实体栏列。与方块栏同理：中文名在前，ID 保留。 */
  const entityColumns: TableColumnData[] = [
    { title: "中文名", slotName: "zhName", width: 150, ellipsis: true, tooltip: true },
    { title: "实体ID", dataIndex: "id" },
    { title: "数量", dataIndex: "count", width: 90 },
  ];

  return {
    viewer,
    scene,
    sceneLoading,
    sceneError,
    brightness,
    activeView,
    exportSize,
    exporting,
    exportingProgress,
    loadScene,
    sceneInfo,
    setView,
    exportPreview,
    pickExportDir,
    exportBatch,
    blockColumns,
    blockColWidths,
    blockTableKey,
    onBlockColumnResize,
    blockTableColumns,
    resetBlockColumnWidths,
    BLOCK_PAGE_SIZE,
    blockPage,
    blockRows,
    blockPageRows,
    blockStateText,
    iconOf,
    iconErrorOf,
    resetBlockIcons,
    blockTableText,
    exportBlockTable,
    copyBlockTable,
    entityColumns,
  };
}

export type ScenePreviewSlice = ReturnType<typeof createScenePreviewSlice>;
