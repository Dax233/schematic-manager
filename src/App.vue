<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ImportOutcome, ModsImport, PackEntry, PackLoadFailure } from "./types";
import { writeFile } from "@tauri-apps/plugin-fs";
import type { TableColumnData, TableData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import {
  analyzeSchematic,
  applyReplace,
  copyFilesToClipboard,
  copyFilesToDir,
  deleteFiles,
  deletePack,
  loadUiState,
  saveUiState,
  importJar,
  importModsFolder,
  listBlockVersions,
  listPacks,
  openDirectory,
  revealInFileManager,
  setPackEnabled,
  previewReplace,
  buildScene,
  deleteBackup,
  listBackups,
  analyzeConversion,
  applyConversion,
  convertTargets,
  readOperationLog,
  renderBlockIcons,
  scanDuplicates,
  restoreBackup,
  saveMetadata,
  scanLitematics,
  searchBlocks,
} from "./api";
import SceneViewer from "./components/SceneViewer.vue";
import { OffscreenRenderer, EXPORT_SIZES, VIEW_PRESETS } from "./render/scene3d";
import { NamePool, joinPath, previewStem } from "./render/export_name";
import type {
  AnalysisResult,
  BackupEntry,
  BlockCount,
  BlockHit,
  ConvertItem,
  ConvertTarget,
  DuplicateGroup,
  DuplicateScan,
  ReplaceRule,
  SceneData,
  SchematicSummary,
  VersionOption,
} from "./types";
import { formatDims, formatSize, formatStacks } from "./utils/format";

const appWindow = getCurrentWindow();

/**
 * 跟随系统深浅色。
 *
 * Arco 的深色变量挂在 `body[arco-theme="dark"]` 上，而样式表里两套取值都
 * 已经写好（见 styles/theme.scss 与自定义样式里的深色分支）——但没人设置
 * 这个属性的话，那些深色取值永远不会生效：系统开了深色，界面仍是白的。
 * 这里把窗口的实际主题写到 body 上，并在系统切换时跟随。
 *
 * 读不到主题时（例如在浏览器里跑前端、或平台不提供该能力）保持浅色，
 * 不影响其余功能。
 */
async function followSystemTheme() {
  try {
    const apply = (t: string | null) => {
      if (t === "dark") document.body.setAttribute("arco-theme", "dark");
      else document.body.removeAttribute("arco-theme");
    };
    apply(await appWindow.theme());
    await appWindow.onThemeChanged(({ payload }) => apply(payload));
  } catch (e) {
    // 不静默吞掉：这类失败会让界面主题与系统不一致，需要留下线索
    console.warn("读取系统主题失败，按浅色显示：", e);
  }
}

/** 每个已加载目录 → 该目录的扫描结果。
 *
 * 用「目录 → 结果」而不是一个扁平列表：用户可一次加载多个目录，
 * 移除其中一个时只需丢掉它那一份，不必重新扫描其余目录。
 *
 * 键是用户选择的目录本身（不是文件所在的子目录）：
 * 一个目录下的文件可能分布在多层子目录里，用文件自身的 dirName 分不出
 * 「它属于哪次加载」。
 */
const dirItems = ref<Record<string, SchematicSummary[]>>({});

/** 已加载的目录，按加载顺序 */
const loadedDirs = computed(() => Object.keys(dirItems.value));

/** 在下拉中被勾选（即参与列表显示）的目录。
 *
 * 新加载的目录默认勾选：用户刚选完目录，期望立刻看到内容。
 * 用 Set 便于 O(1) 判断，且与「哪些目录被加载」分开——
 * 加载了但不勾选，就是这个目录的文件暂不显示。
 */
const enabledDirs = ref<Set<string>>(new Set());

/** 目录被勾选的依据：已加载且勾选，外部加进来的目录一律不显示 */
function isDirEnabled(dir: string): boolean {
  return enabledDirs.value.has(dir);
}

/** 勾选/取消一个目录 */
function toggleDir(dir: string, on: boolean) {
  const next = new Set(enabledDirs.value);
  if (on) next.add(dir);
  else next.delete(dir);
  enabledDirs.value = next;
  // 该目录的文件可能正被选中或勾选，需要一并收敛，
  // 否则详情会停留在已不显示的文件上
  syncSelection(files.value);
  pruneChecks(files.value);
}

/** 全部目录合并后的文件列表，只含被勾选目录的文件，按路径排序保证顺序稳定 */
const files = computed<SchematicSummary[]>(() => {
  const all: SchematicSummary[] = [];
  for (const d of loadedDirs.value) {
    if (!enabledDirs.value.has(d)) continue;
    all.push(...(dirItems.value[d] ?? []));
  }
  all.sort((a, b) => a.path.localeCompare(b.path));
  return all;
});

const selected = ref<SchematicSummary | null>(null);
const analysis = ref<AnalysisResult | null>(null);
const analyzing = ref(false);
const saving = ref(false);
// 元数据编辑草稿。选中文件时用文件中的值填充
const draft = ref({ name: "", author: "", description: "" });
const dirty = ref(false);
const searchText = ref("");

/** 名称列的默认宽度。取实测自然宽度（176px）的两倍。
 *
 * 名称是最需要宽度的列：投影文件名普遍较长，之前一律被省略号截断，
 * 只能靠悬停提示看全名。加倍后同屏能直接读出更多字符。
 *
 * 声明在目录栏默认宽度之前：后者要用它算出「能容下全部列」的宽度。
 */
const NAME_COL_W = 352;

/** 左侧目录栏宽度（像素）。
 *
 * 用户可以拖动中间的拖动条调整，因此不写死在样式里。
 * 取值同时受 MIN/MAX 约束：太窄会让「名称/制作版本/尺寸」三列挤在一起、
 * 太宽则把右侧预览与详情压得过小。
 *
 * 默认值必须容得下各列默认宽度之和（见 SIDEBAR_FIT_W），
 * 否则最后一列「来源目录」会被挤出可视区——名称加宽本意是
 * 「显示更多内容」，挤出末列就与这个目的相反了。
 */
const SIDEBAR_MIN_W = 320;
const SIDEBAR_MAX_W = 1200;
/** 各列默认宽度之和，含选择列与 1px 占位列。改列宽时要同步。 */
const SIDEBAR_FIT_W = 40 + NAME_COL_W + 90 + 110 + 73 + 1;
/** 目录栏默认宽度。
 *
 * 取参考截图的实测值：截图宽 2047 设备像素、缩放 1.25（由已知的标题栏 32px
 * 与工具栏 44px 反推），表头里的竖直分隔线在设备像素 52/492/605/742/833，
 * 换算成 CSS 像素是 41.6 / 393.6 / 484 / 593.6 / 666.4，
 * 即各列 40（选择）/ 352 / 90 / 110 / 73，合计正好 666。
 *
 * 仍保留 SIDEBAR_FIT_W 兜底：万一以后把某列调宽到放不下，
 * 目录栏跟着变宽，而不是把末列挤出可视区。 */
const SIDEBAR_DEFAULT_W = Math.max(666, SIDEBAR_FIT_W);
const sidebarWidth = ref(SIDEBAR_DEFAULT_W);
/** 是否正在拖动分隔条。拖动期间禁用过渡并屏蔽文本选择 */
const resizing = ref(false);

/// 夹到合法区间，避免拖动或窗口变化后得到不可用宽度
function clampSidebar(w: number): number {
  const maxByWindow = Math.max(SIDEBAR_MIN_W, window.innerWidth - 360);
  return Math.min(Math.max(Math.round(w), SIDEBAR_MIN_W),
    Math.min(SIDEBAR_MAX_W, maxByWindow));
}

/// 从指针事件取横坐标并更新宽度
function resizeTo(clientX: number) {
  sidebarWidth.value = clampSidebar(clientX);
}

function onSplitterMove(e: PointerEvent) {
  if (!resizing.value) return;
  resizeTo(e.clientX);
}

function stopResize() {
  if (!resizing.value) return;
  resizing.value = false;
  window.removeEventListener("pointermove", onSplitterMove);
  window.removeEventListener("pointerup", stopResize);
  window.removeEventListener("pointercancel", stopResize);
}

/** 开始拖动分隔条。
 *
 * 监听挂在 window 上而不是分隔条自身：指针移出那条 5px 宽的窄条后
 * 事件仍会继续送到 window，拖动才不会中途断掉。
 */
function startResize(e: PointerEvent) {
  // 只响应左键，避免右键菜单等操作把界面拖乱
  if (e.button !== 0) return;
  e.preventDefault();
  resizing.value = true;
  window.addEventListener("pointermove", onSplitterMove);
  window.addEventListener("pointerup", stopResize);
  window.addEventListener("pointercancel", stopResize);
}

/** 双击分隔条：在默认宽度与最窄宽度之间切换，便于快速让出空间 */
function toggleSidebarWidth() {
  sidebarWidth.value = sidebarWidth.value <= SIDEBAR_MIN_W + 8
    ? clampSidebar(SIDEBAR_DEFAULT_W)
    : SIDEBAR_MIN_W;
}

/** 键盘可用的分隔条：左右方向键调整宽度。
 *
 * 分隔条是 role="separator" 的可聚焦元素，必须能用键盘操作，
 * 只支持鼠标拖动会让键盘用户无法调整布局。
 */
function onSplitterKey(e: KeyboardEvent) {
  const step = e.shiftKey ? 40 : 8;
  if (e.key === "ArrowLeft") {
    sidebarWidth.value = clampSidebar(sidebarWidth.value - step);
  } else if (e.key === "ArrowRight") {
    sidebarWidth.value = clampSidebar(sidebarWidth.value + step);
  } else if (e.key === "Home") {
    sidebarWidth.value = SIDEBAR_MIN_W;
  } else if (e.key === "End") {
    sidebarWidth.value = clampSidebar(SIDEBAR_MAX_W);
  } else {
    return;
  }
  e.preventDefault();
}

/** 窗口变窄时把过宽的目录栏收回来，否则右侧会被挤没 */
function onWindowResize() {
  const clamped = clampSidebar(sidebarWidth.value);
  if (clamped !== sidebarWidth.value) sidebarWidth.value = clamped;
}

onMounted(() => {
  window.addEventListener("resize", onWindowResize);
  // 右键菜单：点击别处、滚动、按 Esc 都要关闭，否则菜单会一直挂在屏幕上
  window.addEventListener("pointerdown", onGlobalPointerDown);
  window.addEventListener("keydown", onGlobalKey);
  window.addEventListener("blur", onGlobalPointerDown);
  // 拦截浏览器默认右键菜单（capture 阶段，确保在组件处理之前生效）
  document.addEventListener("contextmenu", onDocumentContextMenu, true);
  onWindowResize();
  // 恢复上次的界面状态与已添加目录。放在最后：先把界面建起来，
  // 恢复只改状态，不必等它完成就能显示
  void restoreUiState();
});

// 组件卸载时摘掉 window 上的监听，避免拖动中关闭窗口而留下悬挂的监听
onBeforeUnmount(() => {
  window.removeEventListener("resize", onWindowResize);
  window.removeEventListener("pointerdown", onGlobalPointerDown);
  window.removeEventListener("keydown", onGlobalKey);
  window.removeEventListener("blur", onGlobalPointerDown);
  document.removeEventListener("contextmenu", onDocumentContextMenu, true);
  stopResize();
});

// 版本转换
const convOpen = ref(false);
const convBusy = ref(false);
const convTargets = ref<ConvertTarget[]>([]);
const convTargetIdx = ref<number | undefined>(undefined);
const convItems = ref<ConvertItem[]>([]);
const convAnalyzed = ref(false);

const convColumns = [
  { title: "原方块", dataIndex: "name", ellipsis: true, tooltip: true },
  { title: "数量", dataIndex: "count", width: 90 },
  { title: "替代为", slotName: "repl" },
];

async function openConvert() {
  convOpen.value = true;
  convItems.value = [];
  convAnalyzed.value = false;
  convTargetIdx.value = undefined;
  if (!selected.value) return;
  try {
    convTargets.value = await convertTargets(selected.value.path);
  } catch (e) {
    Message.error(`读取可转换版本失败：${e}`);
    convTargets.value = [];
  }
}

async function runConvertAnalyze() {
  const cur = selected.value;
  if (!cur || convTargetIdx.value === undefined) {
    Message.warning("请先选择目标版本");
    return;
  }
  convBusy.value = true;
  try {
    convItems.value = await analyzeConversion(cur.path, convTargetIdx.value);
    convAnalyzed.value = true;
    const missing = convItems.value.filter((i) => !i.chosen && !i.suggested).length;
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
  const cur = selected.value;
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
    await rescanAll();
    syncSelection(files.value);
    pruneChecks(files.value);
  } catch (e) {
    Message.error(`转换失败：${e}`);
  } finally {
    convBusy.value = false;
  }
}

// 重复文件筛查
//
// 按用户要求改为**内联在主界面**：不再单独开弹窗，直接筛查地址栏里
// 已勾选的那些目录，并在列表里把重复文件标成同色、默认勾选。
const dupBusy = ref(false);
const dupResult = ref<DuplicateScan | null>(null);

/** 每个重复文件的路径 -> 组序号（从 1 起）。
 *
 * 组序号用于给同组文件上同一种颜色：界面上一眼看得出「这几个是一组」。
 * 用 Map 而不是逐行查数组：列表可能有上千行，逐行线性查找会退化成平方复杂度。
 */
const dupGroupNo = computed(() => {
  const m = new Map<string, number>();
  (dupResult.value?.groups ?? []).forEach((g, i) => {
    for (const p of g.paths) m.set(p, i + 1);
  });
  return m;
});

/** 重复文件总数（同一份内容重复计数） */
const dupFileCount = computed(() => dupGroupNo.value.size);

/** 执行内联筛查：扫描当前已勾选的目录。
 *
 * 逐目录调用后端再合并，而不是把一批路径传下去：
 * `scan_duplicates` 的契约就是「扫描一个目录」，改成批量会牵动命令层与
 * 现成的用例；而目录数量通常只有个位数，逐次调用的开销可以忽略。
 */
async function scanDuplicatesInline() {
  const dirs = loadedDirs.value.filter((d) => enabledDirs.value.has(d));
  if (dirs.length === 0) {
    Message.warning("请先添加并勾选要筛查的目录");
    return;
  }
  dupBusy.value = true;
  try {
    const merged: DuplicateScan = { groups: [], unreadable: [] };
    for (const d of dirs) {
      const r = await scanDuplicates(d);
      merged.groups.push(...r.groups);
      merged.unreadable.push(...r.unreadable);
    }
    // 跨目录也可能重复（同一份投影被复制到两个目录），因此按大小重新归并：
    // 直接拼接会把「跨目录的同一份内容」拆成两组，用户就看不到它们其实相同。
    merged.groups = mergeDuplicateGroups(merged.groups);
    dupResult.value = merged;

    // 默认勾选「多于重复文件」——用户原话「默认勾选多于重复文件」，
    // 即每组保留第一份、其余勾上，便于直接删除多余的副本。
    applyDefaultDupChecks();
    const n = merged.groups.length;
    Message.success(
      n > 0 ? `发现 ${n} 组重复文件，已默认勾选多余的副本` : "没有发现重复文件",
    );
  } catch (e) {
    Message.error(`筛查失败：${e}`);
    dupResult.value = null;
  } finally {
    dupBusy.value = false;
  }
}

/** 把按目录分别得到的组按内容归并。
 *
 * 归并依据是**大小 + 逐字节相同**：先按大小分桶，再用组内已有的路径
 * 两两比对确认，避免把「大小相同但内容不同」误并成一组。
 * 后端已经保证每组内部一致，因此只需在组之间比对。
 */
function mergeDuplicateGroups(groups: DuplicateGroup[]): DuplicateGroup[] {
  const bySize = new Map<number, DuplicateGroup[]>();
  for (const g of groups) {
    const list = bySize.get(g.size);
    if (list) list.push(g);
    else bySize.set(g.size, [g]);
  }
  const out: DuplicateGroup[] = [];
  for (const [, list] of bySize) {
    const merged: DuplicateGroup[] = [];
    for (const g of list) {
      // 与已归并的组比对：路径集合完全相同才算同一组
      const hit = merged.find((m) => m.paths.some((p) => g.paths.includes(p)));
      if (hit) {
        for (const p of g.paths) if (!hit.paths.includes(p)) hit.paths.push(p);
        hit.paths.sort();
      } else {
        merged.push({ size: g.size, paths: [...g.paths] });
      }
    }
    out.push(...merged);
  }
  out.sort((a, b) => a.paths[0].localeCompare(b.paths[0]));
  return out;
}

/** 默认勾选每组除第一份外的其余副本 */
function applyDefaultDupChecks() {
  const next = new Set(checkedPaths.value);
  for (const g of dupResult.value?.groups ?? []) {
    // 第一份视为「保留的」，其余默认勾上待删
    for (const p of g.paths.slice(1)) next.add(p);
  }
  checkedPaths.value = next;
}

/** 这一行的重复组序号；不重复时为 0 */
function dupNoOf(path: string): number {
  return dupGroupNo.value.get(path) ?? 0;
}

/** 清除重复标记：恢复普通行样式；用户已有的勾选保持不变 */
function clearDuplicateScan() {
  dupResult.value = null;
}

/// 按名称、作者、文件名、路径过滤；同时匹配中英文输入不做转换，
/// 保持与输入一致即可——投影命名本身就是混合的。
const filteredFiles = computed(() => {
  const q = searchText.value.trim().toLowerCase();
  if (!q) return files.value;
  return files.value.filter((f) => {
    const hay = [f.fileName, f.path, f.name ?? "", f.author ?? ""]
      .join("\n")
      .toLowerCase();
    return hay.includes(q);
  });
});
const activeBlock = ref<{ name: string } | null>(null);

// 3D 预览
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

// 备份与日志
const backupOpen = ref(false);
const backups = ref<BackupEntry[]>([]);
const backupLoading = ref(false);
const logs = ref<string[]>([]);
const logLoading = ref(false);

// 方块替换
const replaceOpen = ref(false);
const replaceBusy = ref(false);
const blockHits = ref<BlockHit[]>([]);
const blockSearching = ref(false);
const blockVersions = ref<VersionOption[]>([]);
const replaceForm = ref({
  fromName: "",
  fromPropsText: "",
  toName: "",
  toPropsText: "",
  inheritProps: true,
});
const replacePreview = ref<number | null>(null);

/** 解析 "key=value,key2=value2" 形式的属性限定；空串表示不限定 */
function parseProps(text: string): [string, string][] {
  const out: [string, string][] = [];
  for (const seg of text.split(",")) {
    const t = seg.trim();
    if (!t) continue;
    const i = t.indexOf("=");
    if (i <= 0) continue;
    out.push([t.slice(0, i).trim(), t.slice(i + 1).trim()]);
  }
  return out;
}

function currentRule(): ReplaceRule {
  const f = replaceForm.value;
  return {
    fromName: f.fromName.trim(),
    fromProps: parseProps(f.fromPropsText),
    toName: f.toName.trim(),
    toProps: parseProps(f.toPropsText),
    inheritProps: f.inheritProps,
  };
}

async function onBlockSearch(text: string) {
  const q = text.trim();
  if (!q) {
    blockHits.value = [];
    return;
  }
  blockSearching.value = true;
  try {
    blockHits.value = await searchBlocks(q, 200);
  } catch (e) {
    Message.error(`检索方块失败：${e}`);
  } finally {
    blockSearching.value = false;
  }
}

function onBlockRowClick(raw: TableData) {
  const rec = raw as unknown as BlockCount;
  activeBlock.value = { name: rec.name };
  replaceForm.value.fromName = rec.name;
  // 状态串形如 minecraft:oak_stairs[facing=east,half=top]，
  // 方括号内的部分正好可以直接作为源属性限定
  const m = /\[(.+)\]$/.exec(rec.state ?? "");
  replaceForm.value.fromPropsText = m ? m[1] : "";
  Message.info(`已设为替换源：${rec.name}`);
}

async function openReplace() {
  replaceOpen.value = true;
  replacePreview.value = null;
  blockHits.value = [];
  // 源方块默认取当前选中的方块，省去用户重打一遍
  if (!replaceForm.value.fromName && activeBlock.value) {
    replaceForm.value.fromName = activeBlock.value.name;
  }
  if (blockVersions.value.length === 0) {
    try {
      blockVersions.value = await listBlockVersions();
    } catch (e) {
      Message.error(`读取版本列表失败：${e}`);
    }
  }
}

async function doPreview() {
  const cur = selected.value;
  if (!cur) return;
  const rule = currentRule();
  if (!rule.fromName || !rule.toName) {
    Message.warning("源方块与目标方块都不能为空");
    return;
  }
  replaceBusy.value = true;
  try {
    const r = await previewReplace(cur.path, rule);
    replacePreview.value = r.replaced;
  } catch (e) {
    Message.error(`预览失败：${e}`);
  } finally {
    replaceBusy.value = false;
  }
}

async function doReplace() {
  const cur = selected.value;
  if (!cur) return;
  const rule = currentRule();
  if (!rule.fromName || !rule.toName) {
    Message.warning("源方块与目标方块都不能为空");
    return;
  }
  replaceBusy.value = true;
  try {
    const r = await applyReplace(cur.path, rule);
    if (r.replaced === 0) {
      Message.info("没有方块需要替换");
    } else {
      Message.success(`已替换 ${r.replaced} 个方块（改动前已备份）`);
      replaceOpen.value = false;
      await rescanAll();
      const hit = files.value.find((f) => f.path === cur.path);
      if (hit) {
        selected.value = hit;
        resetDraft(hit);
        const idx = files.value.findIndex((f) => f.path === hit.path);
        if (idx >= 0) files.value[idx] = hit;
        void loadScene(hit);
      }
    }
  } catch (e) {
    Message.error(`替换失败：${e}`);
  } finally {
    replaceBusy.value = false;
  }
}
const sceneInfo = computed(() => {
  if (!scene.value) return "";
  const s = scene.value;
  const parts = [`${s.totalTriangles.toLocaleString()} 个三角形`];
  if (s.unresolvedBlocks > 0) parts.push(`${s.unresolvedBlocks} 种方块无贴图`);
  return parts.join("　");
});
const status = ref("就绪");

/** 目录栏表格的列（默认宽度）。
 *
 * 用户可在表头拖动调整宽度（见 columnResizable）；调整结果存在
 * colWidths 里，由 tableColumns 套回，因此这里始终是「默认值」。
 *
 * 「尺寸」列原本只有 slotName。Arco 的列宽拖动只对有 dataIndex 的列生效
 * （table.js 里 resizable 要求 column.dataIndex），因此这里补一个 dataIndex——
 * td 渲染时 slotName 优先（实测 table-td.js:148），显示内容不受影响，
 * 只是让该列也能拖动。
 */
const columns = computed<TableColumnData[]>(() => [
  { title: "名称", dataIndex: "fileName", width: NAME_COL_W, ellipsis: true, tooltip: true },
  { title: "制作版本", dataIndex: "mcVersionName", width: 90 },
  // 110 是实测值：`16 × 12 × 16` 在 100px 下会折成两行
  { title: "尺寸", dataIndex: "dims", slotName: "dims", width: 110 },
  // 方块总数收窄到参考图的 73px：这几列加起来正好等于参考里的目录栏宽度 666
  { title: "方块总数", dataIndex: "totalBlocks", width: 73 },
  // 1px 占位列，只为让最后一列（方块总数）也能拖动。
  //
  // Arco 判定可拖动时要求 `index2 < row.length - 1`（table.js:1247）：
  // **最后一列永远不能拖**，因为它的宽度被当成「剩余空间」。
  // 而用户要求每一列都能调宽。补一个 1px 宽的占位列放在最右侧，
  // 它就不再是最后一列，从而获得拖动手柄；
  // 占位列自身没有 dataIndex，本来也不参与拖动，且被样式隐藏。
  //
  // Arco 的列 class 属性是 headerCellClass / bodyCellClass，**没有 className**
  // （见 interface.d.ts）。原先写 className 会被静默忽略，占位列因此仍带边框。
  {
    title: "",
    dataIndex: "__filler",
    width: 1,
    headerCellClass: "sidebar__filler-col",
    bodyCellClass: "sidebar__filler-col",
  },
]);

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
  { title: "状态", dataIndex: "state", ellipsis: true, tooltip: true },
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

/** 当前页的行。渲染图只给可见的这批，整表一次性加载会有几 MB 的 data URL。 */
const blockPageRows = computed<BlockCount[]>(() => {
  const all = analysis.value?.blocks ?? [];
  const start = (blockPage.value - 1) * BLOCK_PAGE_SIZE;
  return all.slice(start, start + BLOCK_PAGE_SIZE);
});

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
async function ensureBlockIcons(rows: BlockCount[]) {
  const dataVersion = selected.value?.dataVersion ?? 0;
  if (dataVersion <= 0) return;
  const missing = rows
    .map((r) => r.state)
    .filter((s) => !iconCache.has(s) && !iconErrors.has(s));
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
      for (const s of chunk) iconErrors.set(s, reason);
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
 */
function blockTableText(): string {
  const rows: string[] = [];
  rows.push(["中文名", "方块ID", "状态", "数量", "堆叠"].join("\t"));
  for (const b of analysis.value?.blocks ?? []) {
    const name = (b.zhName || b.enName || "").replace(/[\t\r\n]/g, " ");
    const state = b.state.replace(/[\t\r\n]/g, " ");
    rows.push([name, b.name, state, String(b.count), formatStacks(b.count)].join("\t"));
  }
  return rows.join("\r\n");
}

/** 把方块统计导出为 .txt 文件（用户可选保存位置） */
async function exportBlockTable() {
  const blocks = analysis.value?.blocks ?? [];
  if (blocks.length === 0) {
    Message.warning("当前没有方块统计可导出");
    return;
  }
  const target = await save({
    defaultPath: `${selected.value?.fileName?.replace(/\.litematic$/i, "") ?? "方块统计"}-方块表.txt`,
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
  const blocks = analysis.value?.blocks ?? [];
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

const statusText = computed(() => {
  const n = files.value.length;
  return n === 0 ? status.value : `${status.value}　共 ${n} 个投影文件`;
});

/// 列表被新的扫描结果替换后，让选中项与新列表保持一致。
///
/// 选中项记录的是上一次扫描的路径。若只替换列表而不处理选中项，详情面板
/// 会继续显示**已经不在列表中**的文件：「保存元数据」「方块替换」
/// 「版本转换」都会作用到那个路径上，而用户在列表里选中不了它，
/// 看不出自己的改动写到了哪里。
///
/// 按路径比对而不是一律清空：在同一目录重新扫描时该文件仍在列表中，
/// 此时应保留选中并刷新记录（元数据与解析错误都可能已经变化）。
/// 这与 onRowClick 的行为一致——切换查看对象时草稿一律重置。
function syncSelection(items: SchematicSummary[]) {
  const cur = selected.value;
  if (!cur) return;
  const hit = items.find((f) => f.path === cur.path);
  if (hit) {
    selected.value = hit;
    resetDraft(hit);
    return;
  }
  // 文件已不在新列表中：清空选中，以及只属于它的解析结果与预览，
  // 否则右侧面板会显示上一个目录的数据
  selected.value = null;
  resetDraft(null);
  analysis.value = null;
  scene.value = null;
  sceneError.value = "";
}

// ---------------------------------------------------------------- 目录栏多选与文件操作

/** 已勾选的文件路径集合。
 *
 * 用 Set 而不是数组：勾选/取消/反选都只做 O(1) 的成员判断，
 * 而 129 个文件时每次全量比较数组会明显变慢。
 * 同时与列表数据解耦——列表重新扫描后，仍然存在的路径自然保持勾选。
 */
const checkedPaths = ref<Set<string>>(new Set());

/** 供 a-table 的 rowSelection 使用：Arco 要求数组形式的 keys */
const checkedKeys = computed(() => Array.from(checkedPaths.value));

/** 当前列表里被勾选的条数（只算可见的，避免「筛掉了却仍算选中」） */
const checkedCount = computed(() =>
  filteredFiles.value.filter((f) => checkedPaths.value.has(f.path)).length,
);

/** 被勾选的文件记录（按列表顺序，保证操作顺序稳定可预期） */
const checkedFiles = computed(() =>
  filteredFiles.value.filter((f) => checkedPaths.value.has(f.path)),
);

const hasChecked = computed(() => checkedCount.value > 0);

/** 勾选变化：Arco 传回完整 keys 数组 */
function onSelectionChange(keys: (string | number)[]) {
  checkedPaths.value = new Set(keys.map(String));
}

/** 全选当前列表（只选可见项：搜索过滤后「全选」应当只作用于筛选结果） */
function checkAll() {
  const next = new Set(checkedPaths.value);
  for (const f of filteredFiles.value) next.add(f.path);
  checkedPaths.value = next;
}

/** 反选当前列表：可见项在「选中/未选中」之间翻转 */
function invertChecks() {
  const next = new Set(checkedPaths.value);
  for (const f of filteredFiles.value) {
    if (next.has(f.path)) next.delete(f.path);
    else next.add(f.path);
  }
  checkedPaths.value = next;
}

/** 清除全部勾选（含被搜索过滤掉、当前不可见的那些） */
function clearChecks() {
  checkedPaths.value = new Set();
}

/** 扫描结果更新后丢弃已不存在的勾选项。
 *
 * 必须清理：勾选集里留着已删除/已移走的路径，之后点「删除」或「复制」
 * 会因路径不存在而整批失败，用户却看不出是哪个文件的问题。
 */
function pruneChecks(items: SchematicSummary[]) {
  const alive = new Set(items.map((f) => f.path));
  let changed = false;
  const next = new Set<string>();
  for (const p of checkedPaths.value) {
    if (alive.has(p)) next.add(p);
    else changed = true;
  }
  if (changed) checkedPaths.value = next;
}

// 列宽：由用户在表头拖动调整。
const colWidths = ref<Record<string, number>>({});

/** 表头拖动结束：记录该列的新宽度 */
function onColumnResize(dataIndex: string, width: number) {
  colWidths.value = { ...colWidths.value, [dataIndex]: width };
  // 列宽属于「玩家对界面的修改」，同样要记住
  persistUiState();
}

/** 计算实际用于渲染的列：把用户调整过的宽度套回去。 */
const tableColumns = computed<TableColumnData[]>(() =>
  columns.value.map((c) => {
    const key = String(c.dataIndex ?? "");
    const w = key ? colWidths.value[key] : undefined;
    return w ? { ...c, width: w } : c;
  }),
);

/** 表格的 key。改变它会强制重建表格组件。
 *
 * 复位列宽只能靠重建，原因在 Arco 内部：拖动时它把宽度记在自己的
 * `columnWidth` 里，渲染时按
 *   `columnWidth[dataIndex] || column.width`
 * 取值（table-col-group.js）。也就是说一旦内部记下宽度，外部怎么改
 * `columns.width` 都不会生效——实测拖到 266px 后点复位，仍停在 266px。
 * Arco 没有提供清除该状态的接口，因此改为换 key 重建。
 */
const tableKey = ref(0);

/** 恢复默认列宽：清掉用户改动，并重建表格以清掉 Arco 内部记录 */
function resetColumnWidths() {
  colWidths.value = {};
  tableKey.value += 1;
  persistUiState();
}

const fileBusy = ref(false);

/** 统一的文件操作错误提示：把后端给的原因原样带出来 */
function reportFileOpError(action: string, e: unknown) {
  Message.error(`${action}失败：${e}`);
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

/** 删除勾选的文件（移入回收站） */
async function doDeleteFiles() {
  const list = checkedFiles.value;
  if (list.length === 0) return;
  fileBusy.value = true;
  try {
    const r = await deleteFiles(list.map((f) => f.path));
    Message.success(`已删除 ${r.names.length} 个文件（${formatBytes(r.totalBytes)}），可从回收站还原`);
    // 从各个目录的结果里移除已删除的文件。
    // 按路径而不是文件名移除：不同目录下可能有同名文件，
    // 按文件名会把另一个目录里同名的文件也一并移出列表。
    const gone = new Set(list.map((f) => f.path));
    const next: Record<string, SchematicSummary[]> = {};
    for (const d of loadedDirs.value) {
      next[d] = (dirItems.value[d] ?? []).filter((f) => !gone.has(f.path));
    }
    dirItems.value = next;
    pruneChecks(files.value);
    syncSelection(files.value);
  } catch (e) {
    reportFileOpError("删除", e);
  } finally {
    fileBusy.value = false;
  }
}

/** 复制勾选的文件到剪贴板 */
async function doCopyFiles() {
  const list = checkedFiles.value;
  if (list.length === 0) return;
  fileBusy.value = true;
  try {
    const r = await copyFilesToClipboard(list.map((f) => f.path));
    Message.success(
      `已复制 ${r.names.length} 个文件（${formatBytes(r.totalBytes)}）到剪贴板，可在资源管理器里粘贴`,
    );
  } catch (e) {
    reportFileOpError("复制", e);
  } finally {
    fileBusy.value = false;
  }
}

/** 复制到指定目录：先让用户选目标目录 */
async function doCopyFilesTo() {
  const list = checkedFiles.value;
  if (list.length === 0) return;
  try {
    const dest = await open({ directory: true, multiple: false, title: "选择目标文件夹" });
    if (typeof dest !== "string") return;
    fileBusy.value = true;
    const r = await copyFilesToDir(list.map((f) => f.path), dest);
    Message.success(
      `已复制 ${r.names.length} 个文件（${formatBytes(r.totalBytes)}）到 ${r.destDir}`,
    );
  } catch (e) {
    reportFileOpError("复制到", e);
  } finally {
    fileBusy.value = false;
  }
}

/** 打开第一个勾选文件所在的文件夹。
 *
 * 只处理第一个：资源管理器的「选中」语义只对一个目标有意义，
 * 因此界面上也如实提示用的是哪一个。
 */
async function doOpenFolder() {
  const first = checkedFiles.value[0];
  if (!first) return;
  try {
    await revealInFileManager(first.path);
    Message.success(`已在文件管理器中定位：${first.fileName}`);
  } catch (e) {
    reportFileOpError("打开文件夹", e);
  }
}

/** 右键菜单：只保留「打开文件夹」，作用于右键点中的那一行。
 *
 * 用 a-table 的 rowContextmenu 事件而不是自定义 tr 插槽：
 * 该插槽在 Arco 里不存在（实测只在 tbody 内部渲染 tr），
 * 写上去不会报错但永远不生效。
 *
 * 菜单位置用 fixed 定位到鼠标处；点击别处、窗口失焦或按 Esc 关闭。
 */
const ctxMenu = ref<{ open: boolean; x: number; y: number; path: string; name: string }>({
  open: false,
  x: 0,
  y: 0,
  path: "",
  name: "",
});

function onRowContextMenu(raw: TableData, ev: Event) {
  ev.preventDefault();
  const row = raw as unknown as SchematicSummary;
  const me = ev as MouseEvent;
  // 视口下边缘附近要向上翻，否则菜单会被窗口裁掉
  const menuH = 40;
  const y = me.clientY + menuH > window.innerHeight ? me.clientY - menuH : me.clientY;
  ctxMenu.value = { open: true, x: me.clientX, y, path: row.path, name: row.fileName };
}

function closeRowMenu() {
  if (ctxMenu.value.open) ctxMenu.value = { ...ctxMenu.value, open: false };
}

async function revealRow(row: { path: string; name: string }) {
  closeRowMenu();
  try {
    await revealInFileManager(row.path);
    Message.success(`已在文件管理器中定位：${row.name}`);
  } catch (e) {
    reportFileOpError("打开文件夹", e);
  }
}

/** 全局：点击别处或按 Esc 关闭右键菜单 */
function onGlobalPointerDown() {
  closeRowMenu();
}
function onGlobalKey(e: KeyboardEvent) {
  if (e.key === "Escape") closeRowMenu();
}

/** 全局禁用浏览器默认右键菜单。
 *
 * 不拦截时，右键会弹出 WebView2 自带的那套
 * 「返回 / 刷新 / 另存为 / 打印 / 更多工具」——它是浏览器菜单，
 * 出现在桌面应用里既突兀又无用，还会盖住我们自己的菜单。
 *
 * 这里用前端拦截而不是调 WebView2 的
 * `SetAreDefaultContextMenusEnabled(false)`：后者需要在启动阶段拿到
 * 控制器，失败时只能打日志、用户在界面上看不到任何反馈，而这一步
 * 一旦没生效就完全退回浏览器菜单，属于「静默失效」。前端监听则
 * 不依赖平台接口，且能被自动化测试直接验证。
 *
 * 只在没有自己的菜单要展示时才 preventDefault：text/textarea 上保留
 * 原生菜单（复制/粘贴），否则用户没法在输入框里用右键粘贴。
 */
function onDocumentContextMenu(e: MouseEvent) {
  const el = e.target as HTMLElement | null;
  // 输入类元素保留原生菜单：里面的复制/粘贴是系统能力，我们没实现
  if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
    return;
  }
  e.preventDefault();
}


async function pickDirectory() {
  // 对话框调用本身也可能失败（插件未就绪、系统对话框不可用）。
  // 放在 try 之外会让这个 Promise 静默 reject：用户点了「添加目录」
  // 却什么都没发生，也收不到任何提示。其余对话框调用都在 try 内，
  // 这里保持一致。
  let picked: unknown;
  try {
    picked = await open({
      directory: true,
      multiple: true,
      title: "选择投影所在文件夹（可多选）",
    });
  } catch (e) {
    Message.error(`打开目录选择框失败：${e}`);
    return;
  }

  // multiple: true 时返回数组；某些平台/插件版本可能仍返回单个字符串，
  // 两种都接受，避免「多选后一个都不加载」。
  const dirs: string[] = (Array.isArray(picked) ? picked : [picked]).filter(
    (d): d is string => typeof d === "string" && d.length > 0,
  );
  if (dirs.length === 0) return;

  status.value = "正在扫描…";
  const next = { ...dirItems.value };
  const parts: string[] = [];
  let bad = 0;
  let skippedDirs = 0;
  const skippedPaths: string[] = [];
  let added = 0;

  for (const dir of dirs) {
    try {
      const r = await scanLitematics(dir);
      if (!(dir in next)) added++;
      next[dir] = r.items;
      bad += r.items.filter((f) => f.error).length;
      skippedDirs += r.skippedDirs;
      skippedPaths.push(...r.skippedPaths);
    } catch (e) {
      // 一个目录失败不应让其余目录也加载不了；如实报出是哪个目录
      Message.error(`扫描 ${dir} 失败：${e}`);
    }
  }

  if (Object.keys(next).length === Object.keys(dirItems.value).length && added === 0) {
    // 一个目录都没加进来（全部失败或被取消）
    if (skippedDirs === 0) Message.warning("没有新增任何目录");
    status.value = "扫描完成";
    return;
  }

  dirItems.value = next;
  // 新加载的目录默认勾选：用户刚选完目录，期望立刻看到内容。
  // 已存在的目录保持原勾选状态，不因为重新扫描而被重置。
  const checked = new Set(enabledDirs.value);
  for (const dir of dirs) {
    if (dir in next) checked.add(dir);
  }
  enabledDirs.value = checked;
  // 新的合并列表出来后收敛选中与勾选
  syncSelection(files.value);
  pruneChecks(files.value);

  if (added > 0) parts.push(`新增 ${added} 个目录`);
  if (skippedDirs > 0) {
    parts.push(`已跳过 ${skippedDirs} 个无法读取的目录`);
    // 列出被跳过的路径，否则用户无从判断少了哪些内容
    Message.warning({
      content: `以下目录无法读取，已跳过：\n${skippedPaths.join("\n")}`,
      duration: 8000,
    });
  }
  if (bad > 0) parts.push(`${bad} 个文件解析失败`);
  status.value = parts.length > 0 ? `扫描完成，${parts.join("，")}` : "扫描完成";
}

/** 移除一个已加载目录。
 *
 * 只丢掉它那一份扫描结果，不重新扫描其余目录：重扫又慢又可能因
 * 某个目录临时不可读而连带失败，而用户只是想移掉一个目录。
 */
function removeDir(dir: string) {
  const next = { ...dirItems.value };
  delete next[dir];
  dirItems.value = next;
  // 勾选集里也要去掉它：留着已卸载目录的条目，之后重新加载同名目录时
  // 会「自己就勾上了」，与用户预期不符
  const checked = new Set(enabledDirs.value);
  checked.delete(dir);
  enabledDirs.value = checked;
  // 该目录的文件可能正被选中或勾选，需要一并收敛，
  // 否则右侧详情会停留在已移出列表的文件上
  syncSelection(files.value);
  pruneChecks(files.value);
  Message.info(`已移除目录：${dir}`);
}

/** 重新扫描全部已加载目录，用于文件被改动之后。
 *
 * 逐个目录独立处理：某一个目录临时读不到（被删除、权限变化）时，
 * 保留它原有结果并提示，而不是让整次刷新失败、把其它目录也清空。
 */
async function rescanAll(): Promise<void> {
  const dirs = loadedDirs.value;
  if (dirs.length === 0) return;
  const next: Record<string, SchematicSummary[]> = {};
  for (const d of dirs) {
    try {
      const r = await scanLitematics(d);
      next[d] = r.items;
    } catch (e) {
      Message.error(`重新扫描 ${d} 失败：${e}`);
      next[d] = dirItems.value[d] ?? [];
    }
  }
  dirItems.value = next;
}

function resetDraft(record: SchematicSummary | null) {
  draft.value = {
    name: record?.name ?? "",
    author: record?.author ?? "",
    description: record?.description ?? "",
  };
  dirty.value = false;
}

async function saveDraft() {
  const cur = selected.value;
  if (!cur) return;
  saving.value = true;
  try {
    const out = await saveMetadata(cur.path, {
      name: draft.value.name,
      author: draft.value.author,
      description: draft.value.description,
    });
    dirty.value = false;
    // 重新读取，确保界面显示的是文件中的真实内容而不是内存里的草稿
    await rescanAll();
    const hit = files.value.find((f) => f.path === cur.path);
    if (hit) {
      selected.value = hit;
      resetDraft(hit);
      const idx = files.value.findIndex((f) => f.path === hit.path);
      if (idx >= 0) files.value[idx] = hit;
      Message.success(`已保存，备份在：${out.backupPath}`);
    } else {
      // 文件被移动或重命名时，界面若继续显示旧记录会误导用户
      Message.warning(
        `已保存，但重新读取时未在该目录找到该文件（可能已被移动或重命名）。备份在：${out.backupPath}`,
      );
    }
  } catch (e) {
    Message.error(`保存失败：${e}`);
  } finally {
    saving.value = false;
  }
}

// Arco 的 row-click 回调给出的是通用的 TableData，
// 这里收窄回业务类型；行数据本就来自 files，字段是一致的。
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

function formatTime(ns: string): string {
  if (!ns) return "—";
  try {
    // 纳秒用 BigInt 解析再转毫秒，避免超出安全整数范围导致时间错乱
    const ms = Number(BigInt(ns) / 1000000n);
    const d = new Date(ms);
    return Number.isNaN(d.getTime()) ? "—" : d.toLocaleString();
  } catch {
    return "—";
  }
}

async function refreshBackups() {
  backupLoading.value = true;
  try {
    backups.value = await listBackups(selected.value?.fileName.replace(/\.litematic$/i, ""));
  } catch (e) {
    Message.error(`读取备份失败：${e}`);
  } finally {
    backupLoading.value = false;
  }
}

async function refreshLogs() {
  logLoading.value = true;
  try {
    logs.value = await readOperationLog(300);
  } catch (e) {
    Message.error(`读取日志失败：${e}`);
  } finally {
    logLoading.value = false;
  }
}

async function onRestore(row: BackupEntry) {
  const cur = selected.value;
  if (!cur) return;
  try {
    await restoreBackup(row.path, cur.path);
    Message.success("已还原");
    await Promise.all([refreshBackups(), refreshLogs()]);
    // 还原后重新读取文件内容与预览，避免界面仍显示还原前的数据
    await rescanAll();
    const hit = files.value.find((f) => f.path === cur.path);
    if (hit) {
      selected.value = hit;
      resetDraft(hit);
      const idx = files.value.findIndex((f) => f.path === hit.path);
      if (idx >= 0) files.value[idx] = hit;
      void loadScene(hit);
    }
  } catch (e) {
    Message.error(`还原失败：${e}`);
  }
}

async function onDelete(row: BackupEntry) {
  try {
    await deleteBackup(row.path);
    Message.success("已删除");
    await Promise.all([refreshBackups(), refreshLogs()]);
  } catch (e) {
    Message.error(`删除失败：${e}`);
  }
}

// ---------------------------------------------------------------- 资源包管理

const packOpen = ref(false);
const packBusy = ref(false);
/** 打开弹窗过程中的加载态，供工具栏按钮显示 */
const packOnOpen = ref(false);
const packs = ref<PackEntry[]>([]);
/** 资源包存放目录；打开它才能看到、清理磁盘上实际存在的文件 */
const packsDir = ref("");
/** 清单里解析失败被跳过的行；必须显示，否则"导入的包不见了"无从解释 */
const packSkipped = ref<string[]>([]);
/**
 * 已启用但载入失败的包。
 *
 * 必须显示：这些包在界面上是启用状态，实际却不参与解析，
 * 用户会困惑于「明明导入了为什么没变化」。
 */
const packLoadFailures = ref<PackLoadFailure[]>([]);
/** 导入类型：1 = MC 版本包，2 = 模组包 */
const importKind = ref<number>(1);
/** 模组包叠加到哪个数据版本：0 表示尚未选择 */
const importDataVersion = ref<number>(0);
const importBusy = ref(false);
/** mods 文件夹批量导入是否进行中：与单个 jar 导入分开，
 * 否则批量导入期间「导入 jar」按钮会一起转圈，用户以为点错了。 */
const modsBusy = ref(false);

/**
 * 可选的叠加目标版本。
 *
 * `list_block_versions` 按 dataVersion **升序**返回，因此末项是最新的。
 * 这里不依赖该顺序去取最大值，避免后端排序一变就选错版本。
 */
const overlayVersions = ref<VersionOption[]>([]);

const packColumns = [
  { title: "名称", dataIndex: "name", ellipsis: true, tooltip: true },
  { title: "类型", slotName: "kind", width: 110 },
  { title: "适用版本", slotName: "version", width: 150 },
  // 来源目录：用户要求「记录导入来源文件夹目录与模组名字」。
  // 名称列已经在显示模组名，这里给出来源目录，两者合起来即可追溯。
  { title: "来源目录", slotName: "sourceDir", ellipsis: true, tooltip: true, width: 220 },
  { title: "大小", slotName: "size", width: 100 },
  { title: "状态", slotName: "state", width: 90 },
  { title: "操作", slotName: "actions", width: 140 },
];

/**
 * 刷新计次。
 *
 * 导入、删除、开关都会触发刷新，而这些操作可能重叠。若不加计次，
 * 先发出的请求后返回时会把较新的结果覆盖掉，界面显示的清单与实际不符。
 * 只接受最后一次请求的结果。
 */
let refreshSeq = 0;

/** 正在处理的资源包文件，用于避免同一项被重复操作 */
const packPending = ref<Set<string>>(new Set());

function markPending(file: string, on: boolean) {
  // Set 就地增删不会触发 Vue 的响应式更新，必须换一个新对象
  const next = new Set(packPending.value);
  if (on) next.add(file);
  else next.delete(file);
  packPending.value = next;
}

async function refreshPacks() {
  const seq = ++refreshSeq;
  packBusy.value = true;
  try {
    const r = await listPacks();
    // 已有更新的请求发出过，丢弃本次结果
    if (seq !== refreshSeq) return;
    packs.value = r.packs;
    packsDir.value = r.packsDir;
    packSkipped.value = r.skipped;
    packLoadFailures.value = r.loadFailures;
    if (r.skipped.length > 0) {
      // 静默丢弃会让用户以为导入的包丢了，必须明确提示
      Message.warning({
        content: `资源包清单有 ${r.skipped.length} 行无法解析：\n${r.skipped.join("\n")}`,
        duration: 10000,
      });
    }
  } catch (e) {
    if (seq === refreshSeq) Message.error(`读取资源包清单失败：${e}`);
  } finally {
    // 只有最后一次请求才能解除加载态，否则先返回的会把后面的加载态清掉
    if (seq === refreshSeq) packBusy.value = false;
  }
}

/** 打开资源包目录，去管理磁盘上的文件 */
async function openPacksDir() {
  const dir = packsDir.value;
  if (!dir) {
    Message.warning("尚未取到资源包目录，请先点刷新");
    return;
  }
  try {
    await openDirectory(dir);
  } catch (e) {
    Message.error(`打开资源包目录失败：${e}`);
  }
}

/** 导入 jar：先选文件，再按当前类型与目标版本导入 */
async function runImportJar() {
  // 置位必须在打开文件对话框之前：对话框返回后才置位的话，
  // 连点两次会开出两个对话框并触发两次导入
  if (importBusy.value) return;
  importBusy.value = true;

  // 类型与目标版本在打开对话框前定下来，之后不再读取响应式状态，
  // 保证「校验用的值」与「实际发出的值」是同一个
  const kind = importKind.value;
  const dv = importDataVersion.value;

  try {
    const picked = await open({
      multiple: false,
      title: kind === 2 ? "选择模组 jar" : "选择 Minecraft 客户端 jar",
      filters:
        kind === 2
          ? [{ name: "模组 / 资源包", extensions: ["jar", "zip"] }]
          : [{ name: "Minecraft 客户端", extensions: ["jar"] }],
    });
    if (typeof picked !== "string") return;

    // 模组包必须指定叠加到哪个版本：jar 内没有这个信息
    if (kind === 2 && dv <= 0) {
      Message.warning("请先选择模组包要叠加到的游戏版本");
      return;
    }

    const out: ImportOutcome = await importJar(picked, kind, kind === 2 ? dv : 0);
    Message.success({
      content:
        `已导入 ${out.name}：${out.blocks} 方块 / ${out.models} 模型 / ${out.textures} 贴图` +
        `（${out.kindLabel}，${out.mcVersion || "版本未知"}，${formatSize(out.size)}）`,
      duration: 6000,
    });
    // 跳过的内容必须告知：否则用户以为导入完整
    const notes: string[] = [];
    if (out.failedBlockstates > 0) {
      notes.push(`${out.failedBlockstates} 个 blockstate 解析失败被跳过`);
    }
    if (out.failedBlocks > 0) {
      notes.push(`${out.failedBlocks} 个方块因模型无法解析被跳过`);
    }
    // 版本包却读不出游戏版本：多半是把模组或非客户端 jar 当成了版本包。
    // 这样的包在版本选择里不会命中任何投影，必须明确提醒。
    if (kind === 1 && out.dataVersion <= 0) {
      notes.push(
        "该 jar 内没有可识别的游戏版本（缺 version.json），" +
          "多半不是客户端 jar；这个包不会参与版本匹配",
      );
    }
    if (notes.length > 0) {
      Message.warning({ content: notes.join("；"), duration: 8000 });
    }
    await refreshPacks();
  } catch (e) {
    Message.error(`导入失败：${e}`);
  } finally {
    importBusy.value = false;
  }
}

/** 从 mods 文件夹批量导入。
 *
 * 用户要求「模组包要选择到 mods 文件夹，导入所有模组方块贴图」。
 * 逐个选 jar 在装了上百个模组时不可行，因此提供整目录导入。
 */
async function runImportModsFolder() {
  if (modsBusy.value) return;
  // 模组包必须指明叠加到哪个版本：jar 内没有这个信息
  const dv = importDataVersion.value;
  if (dv <= 0) {
    Message.warning("请先选择模组包要叠加到的游戏版本");
    return;
  }
  const picked = await open({
    directory: true,
    multiple: false,
    title: "选择 mods 文件夹",
  });
  if (typeof picked !== "string") return;

  modsBusy.value = true;
  try {
    const out: ModsImport = await importModsFolder(picked, dv);
    if (out.imported.length > 0) {
      Message.success({
        content: `已从该文件夹导入 ${out.imported.length} 个模组`,
        duration: 6000,
      });
    }
    // 失败项必须逐条列出：用户需要知道少了哪些模组、为什么
    if (out.failed.length > 0) {
      Message.warning({
        content:
          `${out.failed.length} 个模组导入失败：` +
          out.failed
            .slice(0, 3)
            .map((f) => `${f.path}（${f.reason}）`)
            .join("；") +
          (out.failed.length > 3 ? ` …等 ${out.failed.length} 个` : ""),
        duration: 12000,
      });
    }
    await refreshPacks();
  } catch (e) {
    Message.error(`批量导入失败：${e}`);
  } finally {
    modsBusy.value = false;
  }
}

async function togglePack(rec: PackEntry, enabled: boolean) {
  // 同一项有操作在执行时不再受理，避免两个请求交错后开关与实际不符
  if (packPending.value.has(rec.file)) {
    Message.warning(`${rec.name} 正在处理中，请稍候`);
    return;
  }
  markPending(rec.file, true);
  try {
    await setPackEnabled(rec.file, enabled);
    rec.enabled = enabled;
  } catch (e) {
    // 失败时把开关状态改回去，避免界面与实际不一致
    rec.enabled = !enabled;
    Message.error(`操作失败：${e}`);
  } finally {
    markPending(rec.file, false);
  }
}

async function removePack(rec: PackEntry) {
  if (packPending.value.has(rec.file)) {
    Message.warning(`${rec.name} 正在处理中，请稍候`);
    return;
  }
  markPending(rec.file, true);
  try {
    await deletePack(rec.file);
    Message.success(`已删除 ${rec.name}`);
    await refreshPacks();
  } catch (e) {
    Message.error(`删除失败：${e}`);
  } finally {
    markPending(rec.file, false);
  }
}

/** 打开资源包管理时一并取出可选的叠加版本 */
async function prepareOverlayVersions() {
  if (overlayVersions.value.length > 0) return;
  try {
    // 只保留非叠加版本作为叠加目标：叠加包本身不能再作为另一个叠加包的
    // 目标（语义上不通），而且它与所依附的基础版本共享 dataVersion，
    // 会让下拉项互相冲突
    const all = await listBlockVersions();
    overlayVersions.value = all.filter((v) => !v.isOverlay);
    // 默认选最新版本：模组通常针对较新的版本。
    // 取最大 dataVersion 而不是末元素，去掉对返回顺序的依赖。
    if (importDataVersion.value <= 0 && overlayVersions.value.length > 0) {
      importDataVersion.value = overlayVersions.value.reduce(
        (best, v) => (v.dataVersion > best ? v.dataVersion : best),
        overlayVersions.value[0].dataVersion,
      );
    }
  } catch (e) {
    // 取不到版本列表时模组包无法指定叠加目标，导入会被拒绝；
    // 这不是「仅影响默认值」，必须显式告知，否则用户只会看到导入失败
    Message.error(`读取可用版本失败，模组包暂时无法导入：${e}`);
  }
}

/**
 * 打开资源包管理。
 *
 * 弹窗与清单先出来，可选的叠加版本在后台补：首次打开会触发内置方块库
 * 的载入（解压后约 5.6 MB），若等它完成再开弹窗，用户点了按钮会长时间
 * 没有任何反应。叠加版本只用于模组包的下拉，晚一点到不影响其余操作。
 */
async function openPacksPanel() {
  if (packOpen.value) return;
  // 先置位再取数据，避免连点开出重复请求
  packOpen.value = true;
  packOnOpen.value = true;
  try {
    await Promise.all([refreshPacks(), prepareOverlayVersions()]);
  } finally {
    packOnOpen.value = false;
  }
}

async function openBackups() {
  backupOpen.value = true;
  await Promise.all([refreshBackups(), refreshLogs()]);
}

function setView(name: string) {
  activeView.value = name;
  viewer.value?.applyPreset(name);
}

/** 导出当前视角为透明背景 PNG（单个文件） */
async function exportPreview() {
  const cur = selected.value;
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
 */async function exportBatch() {
  const list = checkedFiles.value.filter((f) => !f.error);
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

/** 列表行的样式类：当前选中项高亮，重复文件按组上色。
 *
 * 用 a-table 的 rowClass 而不是自己拼 tr：行的 DOM 由 Arco 生成，
 * 手改会在排序/筛选重新渲染后丢失。
 *
 * 重复组的颜色类名带序号：CSS 里为 1..6 号各准备一种颜色，超过 6 组
 * 循环使用——同一屏内相邻组不同色即可，不必为几十组各配一色
 * （颜色太多反而分不清）。
 */
function rowClass(raw: TableData): string {
  const rec = raw as unknown as SchematicSummary;
  const cls: string[] = [];
  if (selected.value?.path === rec.path) cls.push("row-selected");
  const no = dupNoOf(rec.path);
  if (no > 0) cls.push(`row-dup-${((no - 1) % 6) + 1}`);
  return cls.join(" ");
}

/** 该行是否属于某个重复组，用于在文件名旁显示标记 */
function isDup(path: string): boolean {
  return dupNoOf(path) > 0;
}

// ---------------------------------------------------------------- 界面状态记忆

/** 会被记住的界面状态。
 *
 * 只记「用户调过的界面形态」，不记业务数据（备份、资源包等有自己的文件）。
 * 版本号用于日后改结构时丢弃旧数据，而不是把不认识的字段硬套上去。
 */
interface UiState {
  version: 1;
  sidebarWidth: number;
  /** 已添加的目录（顺序保持），下次启动自动重新扫描 */
  dirs: string[];
  /** 已勾选显示的目录；缺失时视为全部显示 */
  enabledDirs?: string[];
  /** 用户拖动表头调整过的列宽（dataIndex -> 像素宽度） */
  columnWidths?: Record<string, number>;
  activeView: string;
  brightness: number;
  exportSize: number;
  searchText: string;
}

const UI_STATE_VERSION = 1;

/** 把当前界面形态收集成可保存的对象 */
function collectUiState(): UiState {
  return {
    version: UI_STATE_VERSION,
    sidebarWidth: sidebarWidth.value,
    dirs: loadedDirs.value,
    enabledDirs: Array.from(enabledDirs.value),
    columnWidths: { ...colWidths.value },
    activeView: activeView.value,
    brightness: brightness.value,
    exportSize: exportSize.value,
    searchText: searchText.value,
  };
}

let saveStateTimer: ReturnType<typeof setTimeout> | null = null;

/** 保存界面状态（合并短时间内的多次改动）。
 *
 * 用防抖：拖动分隔条会连续改动侧栏宽度，逐次写盘既无必要也拖慢拖动。
 */
function persistUiState() {
  if (saveStateTimer !== null) clearTimeout(saveStateTimer);
  saveStateTimer = setTimeout(() => {
    saveStateTimer = null;
    void (async () => {
      try {
        await saveUiState(JSON.stringify(collectUiState()));
      } catch (e) {
        // 记不住界面不是致命问题，但要留下线索：静默失败会让用户以为
        // 「记住了」，下次启动发现设置全丢却无从判断
        console.warn("保存界面状态失败：", e);
      }
    })();
  }, 400);
}

/** 恢复界面状态并重新扫描上次的目录 */
async function restoreUiState() {
  let raw: string | null = null;
  try {
    raw = await loadUiState();
  } catch (e) {
    console.warn("读取界面状态失败，使用默认界面：", e);
    return;
  }
  if (!raw) return;

  let st: Partial<UiState>;
  try {
    st = JSON.parse(raw) as Partial<UiState>;
  } catch (e) {
    // 内容损坏时不应让程序起不来，也不该静默：明确告知并按默认界面继续
    console.warn("界面状态内容无法解析，已忽略：", e);
    Message.warning("上次的界面设置无法读取，已恢复默认界面");
    return;
  }
  // 版本不匹配时丢弃：旧结构硬套会得到难以解释的界面
  if (st.version !== UI_STATE_VERSION) {
    console.warn(`界面状态版本为 ${String(st.version)}，当前为 ${UI_STATE_VERSION}，已忽略`);
    return;
  }

  if (typeof st.sidebarWidth === "number") {
    sidebarWidth.value = clampSidebar(st.sidebarWidth);
  }
  if (typeof st.activeView === "string" && st.activeView in VIEW_PRESETS) {
    activeView.value = st.activeView;
  }
  if (typeof st.brightness === "number") {
    brightness.value = Math.min(2, Math.max(0.5, st.brightness));
  }
  if (typeof st.exportSize === "number" && EXPORT_SIZES.includes(st.exportSize as never)) {
    exportSize.value = st.exportSize;
  }
  if (typeof st.searchText === "string") {
    searchText.value = st.searchText;
  }
  // 恢复列宽。只接受正数：0 或负数会让列不可见，损坏的值不该让界面变形
  if (st.columnWidths && typeof st.columnWidths === "object") {
    const clean: Record<string, number> = {};
    for (const [k, v] of Object.entries(st.columnWidths)) {
      if (typeof v === "number" && Number.isFinite(v) && v > 0) clean[k] = v;
    }
    colWidths.value = clean;
  }

  // 重新扫描上次的目录。逐个独立处理：某个目录已被删除或暂时不可读时，
  // 其余目录仍应恢复，并如实说明少了哪个。
  const dirs = Array.isArray(st.dirs) ? st.dirs.filter((d) => typeof d === "string") : [];
  if (dirs.length === 0) return;

  const next: Record<string, SchematicSummary[]> = {};
  const missing: string[] = [];
  for (const dir of dirs) {
    try {
      const r = await scanLitematics(dir);
      next[dir] = r.items;
    } catch {
      // 目录不存在或不可读：这是常见情况（用户移动/删除了目录），
      // 不必报错刷屏，但要把它从记忆里去掉，否则每次启动都会重试
      missing.push(dir);
    }
  }
  dirItems.value = next;
  // 勾选状态跟随恢复；记录里没有的目录（新增的）默认勾选
  const keep = new Set(
    Array.isArray(st.enabledDirs) ? st.enabledDirs.filter((d) => typeof d === "string") : [],
  );
  enabledDirs.value = new Set(
    Object.keys(next).filter((d) => !Array.isArray(st.enabledDirs) || keep.has(d)),
  );
  if (missing.length > 0) {
    Message.warning(`以下目录已无法读取，未恢复：\n${missing.join("\n")}`);
    // 把失效目录从记忆里清掉
    persistUiState();
  }
  status.value = `已恢复 ${Object.keys(next).length} 个目录`;
}

async function onRowClick(raw: TableData) {
  const record = raw as unknown as SchematicSummary;
  selected.value = record;
  resetBlockIcons();
  analysis.value = null;
  resetDraft(record);
  if (record.error) return;
  void loadScene(record);
  analyzing.value = true;
  try {
    analysis.value = await analyzeSchematic(record.path);
  } catch (e) {
    Message.error(`解析失败：${e}`);
  } finally {
    analyzing.value = false;
  }
}

const minimize = () => appWindow.minimize();
const toggleMaximize = () => appWindow.toggleMaximize();
const close = () => appWindow.close();

// 记住界面改动。逐个监听而不是 watch 一个大对象：后者要么深比较整棵状态树，
// 要么在 restoreUiState 写入时也触发一次无谓回写。
//
// 这些 watch 都在模块作用域注册（不在 onMounted 里）：组件挂载与卸载是
// 一对一的，放在挂载里反而会在重复挂载时叠加监听。
watch(sidebarWidth, persistUiState);
watch(activeView, persistUiState);
watch(brightness, persistUiState);
watch(exportSize, persistUiState);
watch(searchText, persistUiState);
// 目录集合变化（添加/移除/勾选）同样要记住
watch(dirItems, persistUiState, { deep: false });
watch(enabledDirs, persistUiState, { deep: false });

// 启动即跟随系统主题；不等它完成，免得拖慢首屏
void followSystemTheme();
</script>

<template>
  <div class="app-shell">
    <div class="titlebar" data-tauri-drag-region>
      <div class="titlebar__logo">图</div>
      <div class="titlebar__title">投影文件管理器</div>
      <div class="titlebar__spacer" data-tauri-drag-region />
      <div class="titlebar__btn" @click="minimize">&#x2500;</div>
      <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
      <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
    </div>

    <div class="toolbar">
      <a-button size="small" type="primary" @click="pickDirectory">添加目录</a-button>
      <!-- 已加载目录的下拉：查看当前加载了哪些、并逐个移除。
           不禁用：未加载时点开应看到「尚未加载任何目录」的说明，
           而不是一个点不动的按钮——那样用户不知道是坏了还是没数据。 -->
      <a-dropdown trigger="click">
        <a-button size="small">
          已加载目录（{{ loadedDirs.length }}）
          <icon-down />
        </a-button>
        <template #content>
          <a-doption v-if="loadedDirs.length === 0" disabled>尚未加载任何目录</a-doption>
          <a-doption v-for="d in loadedDirs" :key="d" :value="d">
            <div class="dir-item">
              <!-- 勾选才在下方列表显示 -->
              <a-checkbox
                :model-value="isDirEnabled(d)"
                @change="toggleDir(d, $event as boolean)"
                @click.stop
              />
              <span class="dir-item__name" :title="d">{{ d }}</span>
              <span class="dir-item__count">{{ (dirItems[d] ?? []).length }} 个</span>
              <!-- 移除按钮：用 @click.stop 阻止冒泡，否则点它会同时选中该项、
                   让下拉以为是「选择目录」而不是「移除」 -->
              <a-button
                size="mini"
                type="text"
                status="danger"
                title="从列表移除该目录（不会删除磁盘上的文件）"
                @click.stop="removeDir(d)"
              >
                移除
              </a-button>
            </div>
          </a-doption>
        </template>
      </a-dropdown>
      <a-input-search
        v-model="searchText"
        size="small"
        placeholder="搜索名称/作者/文件名…"
        style="width: 240px"
        allow-clear
      />
      <a-button size="small" :disabled="!selected" @click="openReplace">方块替换</a-button>
      <a-button size="small" :disabled="!selected" @click="openConvert">版本转换</a-button>
      <a-button size="small" :loading="dupBusy" @click="scanDuplicatesInline">
        重复文件筛查
      </a-button>
      <a-button size="small" @click="openBackups">备份管理</a-button>
      <a-button size="small" :loading="packOnOpen" @click="openPacksPanel">
        资源包管理
      </a-button>
      <span class="preview__spacer" />
      <span class="hint-dim">
        共 {{ files.length }} 个投影<span v-if="searchText">，匹配 {{ filteredFiles.length }} 个</span>
      </span>
    </div>

    <div class="workbench" :class="{ 'is-resizing': resizing }">
      <div class="sidebar" :style="{ width: sidebarWidth + 'px' }">
        <!-- 文件操作条：在目录栏顶部、靠左。
             作用于勾选的文件；未勾选时禁用，避免误操作到「看起来选中」的行。 -->
        <div class="sidebar__ops">
          <a-button
            size="mini"
            status="danger"
            :disabled="!hasChecked"
            :loading="fileBusy"
            @click="doDeleteFiles"
          >
            删除
          </a-button>
          <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFiles">
            复制
          </a-button>
          <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFilesTo">
            复制到
          </a-button>
          <a-button size="mini" :disabled="!hasChecked" @click="doOpenFolder">
            打开文件夹
          </a-button>
          <a-button size="mini" @click="resetColumnWidths" title="把各列宽度恢复为默认">
            列宽复位
          </a-button>
        </div>

        <!-- 重复筛查结果条：就地显示在列表上方，不再单开弹窗。
             重复行本身已按组上色（见 rowClass），这里只给汇总与清除入口。 -->
        <div v-if="dupResult" class="sidebar__dups">
          <template v-if="dupResult.groups.length > 0">
            <span class="sidebar__dups-text">
              发现 <b>{{ dupResult.groups.length }}</b> 组重复、
              <b>{{ dupFileCount }}</b> 个文件；已默认勾选多余的副本
            </span>
            <a-button size="mini" @click="clearDuplicateScan">清除标记</a-button>
          </template>
          <template v-else>
            <span class="sidebar__dups-text">没有发现内容相同的文件</span>
            <a-button size="mini" @click="clearDuplicateScan">知道了</a-button>
          </template>
        </div>
        <a-alert
          v-if="dupResult && dupResult.unreadable.length > 0"
          type="warning"
          style="margin: 0 0 4px; flex: 0 0 auto"
        >
          有 {{ dupResult.unreadable.length }} 个文件无法读取，未参与判定
        </a-alert>

        <a-table
          :key="tableKey"
          :columns="tableColumns"
          :data="filteredFiles"
          :pagination="false"
          row-key="path"
          size="small"
          :bordered="{ wrapper: true, cell: false }"
          :scroll="{ y: '100%' }"
          column-resizable
          :row-selection="{ type: 'checkbox', showCheckedAll: false, width: 40 }"
          :selected-keys="checkedKeys"
          :row-class="rowClass"
          @selection-change="onSelectionChange"
          @column-resize="onColumnResize"
          @row-click="onRowClick"
          @row-contextmenu="onRowContextMenu"
        >
          <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
          <template #empty>
            <a-empty description="点击「添加目录」选择投影所在文件夹" />
          </template>
        </a-table>

        <!-- 目录栏底部：只放多选操作 -->
        <div class="sidebar__checks">
          <span class="hint-dim">已选 {{ checkedCount }} / {{ filteredFiles.length }}</span>
          <span class="preview__spacer" />
          <a-button size="mini" :disabled="filteredFiles.length === 0" @click="checkAll">
            全选
          </a-button>
          <a-button size="mini" :disabled="filteredFiles.length === 0" @click="invertChecks">
            反选
          </a-button>
          <a-button size="mini" :disabled="!hasChecked" @click="clearChecks">清除</a-button>
        </div>
      </div>

      <!-- 拖动条：调整左侧目录栏宽度 -->
      <div
        class="splitter"
        role="separator"
        aria-orientation="vertical"
        aria-label="调整目录栏宽度"
        :aria-valuenow="sidebarWidth"
        :aria-valuemin="SIDEBAR_MIN_W"
        :aria-valuemax="SIDEBAR_MAX_W"
        tabindex="0"
        title="拖动调整宽度，双击复位"
        @pointerdown="startResize"
        @dblclick="toggleSidebarWidth"
        @keydown="onSplitterKey"
      />

      <!-- 行的右键菜单：只保留「打开文件夹」 -->
      <div
        v-if="ctxMenu.open"
        class="ctx-menu"
        :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
        role="menu"
        @contextmenu.prevent
      >
        <div
          class="ctx-menu__item"
          role="menuitem"
          tabindex="0"
          @click="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
          @keydown.enter="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
        >
          打开文件夹
        </div>
      </div>

      <div class="detail">
        <div class="preview">
          <!-- 预览与信息并排：左边是 3D 视图，右边是该投影的基本信息与元数据。
               用横向 flex 让「信息在预览右侧」这条要求不随窗口宽度漂移。 -->
          <div class="preview__main">
            <div class="preview__stage">
              <a-spin :loading="sceneLoading" style="width: 100%; height: 100%">
                <SceneViewer
                  ref="viewer"
                  :scene="scene"
                  :brightness="brightness"
                />
              </a-spin>
            </div>
            <div class="preview__info" v-if="selected">
              <!-- 可滚动区：基本信息 + 三个输入框。操作行在它外面，
                   因此内容再长也不会把保存按钮挤出可视范围。 -->
              <div class="preview__info-body">
              <!-- 只读基本信息。用紧凑的两列排布并缩小字号，
                   把纵向空间让给下面的描述框。 -->
              <dl class="preview__facts">
                <dt>文件名</dt>
                <dd :title="selected.fileName">{{ selected.fileName }}</dd>
                <dt>制作版本</dt>
                <dd :title="`数据版本 ${selected.dataVersion}`">
                  {{ selected.mcVersionName || "未知" }}（数据版本 {{ selected.dataVersion }}）
                </dd>
                <dt>投影格式</dt>
                <dd>v{{ selected.formatVersion }}</dd>
                <dt>尺寸</dt>
                <dd>{{ formatDims(selected.enclosing) }}</dd>
                <dt>区域数量</dt>
                <dd>{{ selected.regionCount }}</dd>
                <dt>方块总数</dt>
                <dd>{{ selected.totalBlocks }}</dd>
                <dt>实体数量</dt>
                <dd>{{ selected.entityCount }}</dd>
              </dl>

              <!-- 可编辑元数据。描述按需求做成多行大输入框 -->
              <a-form :model="draft" layout="vertical" size="small" class="preview__form">
                <a-form-item label="投影名称">
                  <a-input
                    v-model="draft.name"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-form-item label="作者">
                  <a-input
                    v-model="draft.author"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-form-item label="描述（备注）">
                  <a-textarea
                    v-model="draft.description"
                    :auto-size="{ minRows: 6, maxRows: 14 }"
                    placeholder="可多行填写，保存后写入投影文件"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                </a-form>
              </div>
              <!-- 操作行钉在面板底部，不随内容一起滚。
                   面板高度与预览舞台对齐（400px），而「基本信息 + 三个输入框 +
                   多行描述」实测有 513px：不钉住的话保存按钮会落到折叠线以下
                   113px，用户得先滚动才能保存，等于按钮藏起来了。 -->
              <div class="preview__actions">
                <a-button
                  type="primary"
                  size="small"
                  :loading="saving"
                  :disabled="!dirty || !!selected.error"
                  @click="saveDraft"
                >
                  保存
                </a-button>
                <a-button size="small" :disabled="!dirty" @click="resetDraft(selected)">
                  撤销
                </a-button>
                <span v-if="dirty" class="hint-warn">有未保存的修改</span>
                <span v-else-if="!selected.error" class="hint-dim">
                  保存前会自动备份原件
                </span>
              </div>
            </div>
            <div class="preview__info" v-else>
              <a-empty description="选择一个投影文件以查看详情" />
            </div>
          </div>
          <div class="preview__bar">
            <a-radio-group
              v-model="activeView"
              type="button"
              size="small"
              :disabled="!scene"
              @change="setView(String(activeView))"
            >
              <a-radio value="front">正面</a-radio>
              <a-radio value="left45">左侧 45°</a-radio>
              <a-radio value="right45">右侧 45°</a-radio>
              <a-radio value="back">背面</a-radio>
            </a-radio-group>
            <span class="preview__label">亮度</span>
            <a-slider
              v-model="brightness"
              :min="0.5"
              :max="2"
              :step="0.05"
              :style="{ width: '140px' }"
            />
            <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
            <!-- 导出尺寸：四档可选，单个与批量共用。
                 选项来自 EXPORT_SIZES，避免与 scene3d.ts 的定义各写一份而漂移 -->
            <a-select v-model="exportSize" size="small" style="width: 104px">
              <a-option v-for="s in EXPORT_SIZES" :key="s" :value="s">{{ s }}</a-option>
            </a-select>
            <a-button
              size="small"
              type="primary"
              :loading="exporting && !exportingProgress"
              :disabled="!scene || exporting"
              @click="exportPreview"
            >
              导出预览图
            </a-button>
            <!-- 批量导出：对目录栏里勾选的文件各导出一张，输出到所选目录 -->
            <a-button
              size="small"
              :loading="exporting && !!exportingProgress"
              :disabled="!hasChecked || exporting"
              :title="
                hasChecked
                  ? `导出勾选的 ${checkedCount} 个投影各一张预览图`
                  : '先在下方的目录栏里勾选要导出的投影'
              "
              @click="exportBatch"
            >
              批量导出
              <template v-if="exportingProgress">
                （{{ exportingProgress.done }}/{{ exportingProgress.total }}）
              </template>
              <template v-else-if="hasChecked">（{{ checkedCount }}）</template>
            </a-button>
            <span class="preview__spacer" />
            <span v-if="sceneError" class="preview__error">{{ sceneError }}</span>
            <span v-else-if="sceneInfo" class="preview__info">{{ sceneInfo }}</span>
          </div>
        </div>
        <a-tabs
          class="detail__tabs"
          default-active-key="info"
          style="padding: 0 8px"
        >
          <a-tab-pane key="info" title="信息">
            <!-- 只读明细。可编辑的元数据与最常用的几项已移到预览区右侧，
                 这里保留其余的详细信息，避免两处重复显示同一字段。 -->
            <a-descriptions
              v-if="selected"
              :column="1"
              size="small"
              :label-style="{ width: '110px' }"
              bordered
            >
              <a-descriptions-item label="方块实体">{{ selected.tileEntityCount }}</a-descriptions-item>
              <a-descriptions-item label="待处理刻">{{ selected.pendingTicks }}</a-descriptions-item>
              <a-descriptions-item label="内含预览图">
                {{ selected.hasPreview ? "是" : "否" }}
              </a-descriptions-item>
              <a-descriptions-item label="文件大小">{{ formatSize(selected.fileSize) }}</a-descriptions-item>
              <a-descriptions-item label="所在目录">{{ selected.dirName }}</a-descriptions-item>
              <a-descriptions-item v-if="selected.error" label="解析错误">
                <span style="color: rgb(var(--danger-6))">{{ selected.error }}</span>
              </a-descriptions-item>
            </a-descriptions>
            <a-empty v-else description="选择一个投影文件以查看详情" />

          </a-tab-pane>

          <a-tab-pane key="blocks" title="方块">
            <a-spin :loading="analyzing" style="width: 100%; height: 100%">
              <template v-if="analysis">
                <div class="block-head">
                  <a-space :size="16">
                    <span>非空气方块：<b>{{ analysis.nonAirTotal }}</b></span>
                    <span>空气方块：<b>{{ analysis.airTotal }}</b></span>
                    <span>方块种类：<b>{{ analysis.distinctBlockTypes }}</b></span>
                    <span>状态种类：<b>{{ analysis.distinctStates }}</b></span>
                  </a-space>
                  <span class="preview__spacer" />
                  <a-button size="mini" @click="copyBlockTable">复制表格</a-button>
                  <a-button size="mini" @click="exportBlockTable">导出表格</a-button>
                  <a-button
                    size="mini"
                    title="把各列宽度恢复为默认"
                    @click="resetBlockColumnWidths"
                  >
                    列宽复位
                  </a-button>
                </div>
                <div class="hint-dim" style="margin-bottom: 6px">
                  点击任意行可以把它设为方块替换的源方块；表头可拖动调整列宽
                </div>
                <a-table
                  :key="blockTableKey"
                  class="block-table"
                  :columns="blockTableColumns"
                  :data="analysis.blocks"
                  :pagination="{ pageSize: BLOCK_PAGE_SIZE, current: blockPage }"
                  row-key="state"
                  size="small"
                  column-resizable
                  @column-resize="onBlockColumnResize"
                  @page-change="(p: number) => (blockPage = p)"
                  @row-click="onBlockRowClick"
                  :scroll="{ y: '100%' }"
                >
                  <template #icon="{ record }">
                    <img
                      v-if="iconOf(record.state)"
                      class="block-icon"
                      :src="iconOf(record.state)"
                      :alt="record.zhName || record.name"
                      :title="record.zhName || record.name"
                    />
                    <!-- 画不出来时说清楚原因，而不是留一个让人以为是加载慢的空位 -->
                    <span
                      v-else-if="iconErrorOf(record.state)"
                      class="block-icon block-icon--failed"
                      :title="iconErrorOf(record.state)"
                      >无图</span
                    >
                  </template>
                  <template #zhName="{ record }">
                    <span v-if="record.zhName">{{ record.zhName }}</span>
                    <span v-else class="hint-dim" :title="record.enName || '语言表中没有这一项'">
                      {{ record.enName || "—" }}
                    </span>
                  </template>
                  <template #stacks="{ record }">{{ formatStacks(record.count) }}</template>
                </a-table>
              </template>
              <a-empty v-else description="选择投影文件后显示方块统计" />
            </a-spin>
          </a-tab-pane>

          <a-tab-pane key="entities" title="实体">
            <template v-if="analysis">
              <a-table
                :columns="entityColumns"
                :data="analysis.entities"
                :pagination="false"
                row-key="id"
                size="small"
              >
                <template #zhName="{ record }">
                  <span v-if="record.zhName">{{ record.zhName }}</span>
                  <span v-else class="hint-dim" :title="record.enName || '语言表中没有这一项'">
                    {{ record.enName || "—" }}
                  </span>
                </template>
              </a-table>
              <a-empty
                v-if="analysis.entities.length === 0"
                description="该投影不含实体"
              />
            </template>
            <a-empty v-else description="选择投影文件后显示实体统计" />
          </a-tab-pane>
        </a-tabs>
      </div>
    </div>

    <a-modal
      v-model:visible="convOpen"
      title="转换游戏版本"
      :width="760"
      :ok-loading="convBusy"
      ok-text="开始转换"
      :ok-button-props="{ disabled: convTargetIdx === undefined || !selected || !convAnalyzed }"
      @ok="doConvert"
    >
      <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
        请先在左侧选择一个投影文件
      </a-alert>

      <a-space style="margin-bottom: 10px">
        <span>目标版本</span>
        <a-select
          v-model="convTargetIdx"
          placeholder="选择要转换到的版本"
          allow-search
          style="width: 260px"
        >
          <a-option v-for="t in convTargets" :key="t.index" :value="t.index">
            {{ t.name }}（dataVersion {{ t.dataVersion }}）
          </a-option>
        </a-select>
        <a-button size="small" :loading="convBusy" @click="runConvertAnalyze">分析差异</a-button>
      </a-space>

      <div class="hint-dim" style="margin-bottom: 10px">
        转换结果另存为新文件，原文件不会被改动。目标版本里已不存在的方块需要
        在这里指定替代品；「替代为」留空的方块在转换后会保持原样。
      </div>

      <a-spin :loading="convBusy" style="width: 100%">
        <template v-if="convAnalyzed">
          <a-alert v-if="convItems.length === 0" type="success">
            该投影用到的方块在目标版本中都存在，可以直接转换
          </a-alert>
          <a-table
            v-else
            :columns="convColumns"
            :data="convItems"
            :pagination="{ pageSize: 10 }"
            row-key="name"
            size="small"
          >
            <template #repl="{ record }">
              <a-input
                v-model="record.chosen"
                :placeholder="record.suggested || '未找到推荐，请手动填写方块 ID'"
                size="small"
                allow-clear
              />
            </template>
          </a-table>
        </template>
        <a-empty v-else description="请先选择目标版本并分析差异" />
      </a-spin>
    </a-modal>

    <a-modal
      v-model:visible="packOpen"
      title="资源包管理"
      :width="900"
      :footer="false"
      unmount-on-close
    >
      <div class="hint-dim" style="margin-bottom: 12px">
        内置基础库已含 1.13.2 ~ 26.3 的贴图，通常无需手动导入。
        只有在使用模组、或需要更新的正式版贴图时，才需要导入 jar。
      </div>

      <a-space style="margin-bottom: 12px" wrap>
        <a-radio-group v-model="importKind" type="button" size="small">
          <a-radio :value="1">MC 版本包</a-radio>
          <a-radio :value="2">模组包</a-radio>
        </a-radio-group>
        <a-select
          v-if="importKind === 2"
          v-model="importDataVersion"
          size="small"
          style="width: 190px"
          placeholder="叠加到哪个版本"
        >
          <a-option
            v-for="v in overlayVersions"
            :key="v.index"
            :value="v.dataVersion"
          >
            {{ v.name }}
          </a-option>
        </a-select>
        <a-button size="small" type="primary" :loading="importBusy" @click="runImportJar">
          导入 jar…
        </a-button>
        <!-- 整目录导入：装了上百个模组时逐个选 jar 不可行 -->
        <a-button size="small" :loading="modsBusy" @click="runImportModsFolder">
          导入 mods 文件夹…
        </a-button>
        <a-button
          size="small"
          title="在文件管理器里打开资源包存放目录"
          @click="openPacksDir"
        >
          打开资源包目录
        </a-button>
        <a-button size="small" :loading="packBusy" @click="refreshPacks">刷新</a-button>
      </a-space>

      <div class="hint-dim" style="margin-bottom: 10px">
        <template v-if="importKind === 2">
          「导入 mods 文件夹」会一次导入该文件夹下<strong>所有</strong>模组 jar
          （只扫描这一层、不递归），并跳过客户端 jar；
          单个模组失败不会中断整批。
        </template>
        <template v-else>
          选具体的 .jar 文件而不是文件夹：
          <code>.minecraft\versions\&lt;版本&gt;\&lt;版本&gt;.jar</code>
          。要选客户端而不是服务端——服务端没有贴图资源。
        </template>
      </div>

      <a-alert
        v-if="packLoadFailures.length > 0"
        type="error"
        style="margin-bottom: 10px"
      >
        <template #title>
          {{ packLoadFailures.length }} 个已启用的资源包未能载入，它们不会生效
        </template>
        <div v-for="f in packLoadFailures" :key="f.file" class="dup-path">
          {{ f.file }}：{{ f.reason }}
        </div>
        <div style="margin-top: 6px">
          常见原因是文件被手工删除或包本身损坏；重新导入该 jar 即可修复。
        </div>
      </a-alert>

      <a-alert
        v-if="packSkipped.length > 0"
        type="warning"
        style="margin-bottom: 10px"
      >
        <template #title>
          清单有 {{ packSkipped.length }} 行无法解析，这些记录不会被加载
        </template>
        <div v-for="s in packSkipped" :key="s" class="dup-path">{{ s }}</div>
        <div style="margin-top: 6px">
          修好这些行之前，导入、启用与删除都会被拒绝——否则重写清单会把它们
          连同内容一起抹掉。清单位置：&lt;程序目录&gt;\packs\packs.tsv
        </div>
      </a-alert>

      <a-spin :loading="packBusy" style="width: 100%">
        <a-alert v-if="packs.length === 0 && packSkipped.length === 0" type="normal">
          尚未导入任何资源包
        </a-alert>
        <a-alert v-else-if="packs.length === 0" type="warning">
          清单里的记录全部无法解析，请按上方说明修复后再刷新
        </a-alert>
        <a-table
          v-else
          :columns="packColumns"
          :data="packs"
          :pagination="{ pageSize: 8 }"
          size="small"
          row-key="file"
        >
          <template #kind="{ record }">
            <a-tag size="small">{{ record.kindLabel }}</a-tag>
          </template>
          <template #version="{ record }">
            <span>{{ record.mcVersion || "-" }}</span>
          </template>
          <!-- 来源目录：旧清单没有这一列（升级前导入的包），显示占位而不是
               空白，否则用户会以为是界面坏了。 -->
          <template #sourceDir="{ record }">
            <span v-if="record.sourceDir" :title="record.sourceDir">
              {{ record.sourceDir }}
            </span>
            <span v-else class="hint-dim">（旧记录未保存）</span>
          </template>
          <template #size="{ record }">
            <span v-if="record.exists">{{ formatSize(record.size) }}</span>
            <a-tag v-else size="small" color="red">文件缺失</a-tag>
          </template>
          <template #state="{ record }">
            <a-switch
              size="small"
              :model-value="record.enabled"
              :loading="packPending.has(record.file)"
              @change="(v: any) => togglePack(record, Boolean(v))"
            />
          </template>
          <template #actions="{ record }">
            <a-popconfirm
              content="删除后需要重新导入才能恢复，确定删除？"
              @ok="removePack(record)"
            >
              <a-button
                size="mini"
                status="danger"
                :loading="packPending.has(record.file)"
              >
                删除
              </a-button>
            </a-popconfirm>
          </template>
        </a-table>
      </a-spin>
    </a-modal>

    <a-modal
      v-model:visible="replaceOpen"
      title="方块替换"
      :width="640"
      :ok-loading="replaceBusy"
      ok-text="执行替换"
      :ok-button-props="{ disabled: !selected }"
      @ok="doReplace"
    >
      <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
        请先在左侧选择一个投影文件
      </a-alert>
      <a-form :model="replaceForm" layout="vertical" size="small">
        <a-form-item label="源方块（要替换掉的）">
          <a-input v-model="replaceForm.fromName" placeholder="例如 minecraft:oak_stairs" allow-clear />
          <div class="hint-dim">
            在「方块」页签点击任意行可直接填入，属性限定也会一并带入
          </div>
        </a-form-item>
        <a-form-item label="源属性限定（可留空）">
          <a-input
            v-model="replaceForm.fromPropsText"
            placeholder="例如 facing=east,half=top"
            allow-clear
          />
        </a-form-item>

        <a-form-item label="目标方块">
          <a-select
            v-model="replaceForm.toName"
            placeholder="输入中文名、英文名或方块 ID 搜索"
            allow-search
            allow-clear
            :filter-option="false"
            :loading="blockSearching"
            @search="onBlockSearch"
          >
            <a-option v-for="h in blockHits" :key="h.name" :value="h.name">
              <span>{{ h.zh || h.en || h.name }}</span>
              <span class="hint-dim" style="margin-left: 8px">{{ h.name }}</span>
              <span v-if="h.versions.length > 1" class="hint-dim" style="margin-left: 8px">
                {{ h.versions.length }} 个版本
              </span>
            </a-option>
          </a-select>
        </a-form-item>
        <a-form-item label="目标属性覆盖（可选）">
          <a-input
            v-model="replaceForm.toPropsText"
            placeholder="例如 facing=west"
            allow-clear
          />
        </a-form-item>

        <a-form-item label="属性处理">
          <a-space direction="vertical" :size="4">
            <a-checkbox v-model="replaceForm.inheritProps">
              继承源方块中目标也支持的属性
            </a-checkbox>          </a-space>
        </a-form-item>

        <a-form-item>
          <a-space>
            <a-button :loading="replaceBusy" :disabled="!selected" @click="doPreview">
              预计替换数量
            </a-button>
            <span v-if="replacePreview !== null" class="preview__info">
              将替换 <b>{{ replacePreview.toLocaleString() }}</b> 个方块
            </span>
          </a-space>
        </a-form-item>
        <div class="hint-dim">
          方块库共 {{ blockVersions.length }} 个版本可选；替换前会自动备份，可在「备份管理」中还原。
        </div>
      </a-form>
    </a-modal>

    <a-drawer
      v-model:visible="backupOpen"
      title="备份管理"
      :width="620"
      :footer="false"
      unmount-on-close
    >
      <a-tabs default-active-key="backups">
        <a-tab-pane key="backups" :title="`备份（${backups.length}）`">
          <a-space style="margin-bottom: 8px">
            <a-button size="small" @click="refreshBackups">刷新</a-button>
            <span v-if="selected" class="hint-dim">
              仅显示「{{ selected.fileName }}」的备份
            </span>
          </a-space>
          <a-table
            :columns="[
              { title: '时间', slotName: 'time', width: 170 },
              { title: '操作', dataIndex: 'op', width: 110 },
              { title: '大小', slotName: 'size', width: 90 },
              { title: '操作', slotName: 'act', width: 150 },
            ]"
            :data="backups"
            :loading="backupLoading"
            :pagination="{ pageSize: 12 }"
            row-key="path"
            size="small"
          >
            <template #time="{ record }">{{ formatTime(record.createdNs) }}</template>
            <template #size="{ record }">{{ formatSize(record.size) }}</template>
            <template #act="{ record }">
              <a-space>
                <a-link :disabled="!selected" @click="onRestore(record)">还原</a-link>
                <a-popconfirm content="删除后无法恢复，确认删除？" @ok="onDelete(record)">
                  <a-link status="danger">删除</a-link>
                </a-popconfirm>
              </a-space>
            </template>
            <template #empty>
              <a-empty :description="selected ? '该文件暂无备份' : '请先选择一个投影文件'" />
            </template>
          </a-table>
        </a-tab-pane>

        <a-tab-pane key="logs" title="操作日志">
          <a-space style="margin-bottom: 8px">
            <a-button size="small" @click="refreshLogs">刷新</a-button>
          </a-space>
          <a-spin :loading="logLoading" style="width: 100%">
            <a-empty v-if="logs.length === 0" description="暂无日志" />
            <div v-else class="log-list">
              <div v-for="(line, i) in logs" :key="i" class="log-list__row">{{ line }}</div>
            </div>
          </a-spin>
        </a-tab-pane>
      </a-tabs>
    </a-drawer>

    <div class="statusbar">
      <span>{{ statusText }}</span>
      <span class="statusbar__spacer" />
      <span v-if="selected">已选：{{ selected.fileName }}</span>
    </div>
  </div>
</template>
