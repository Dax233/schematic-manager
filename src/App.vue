<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ImportOutcome, PackEntry, PackLoadFailure } from "./types";
import { writeFile } from "@tauri-apps/plugin-fs";
import type { TableColumnData, TableData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import {
  analyzeSchematic,
  applyReplace,
  deletePack,
  importJar,
  listBlockVersions,
  listPacks,
  setPackEnabled,
  previewReplace,
  buildScene,
  deleteBackup,
  listBackups,
  analyzeConversion,
  applyConversion,
  convertTargets,
  readOperationLog,
  scanDuplicates,
  restoreBackup,
  saveMetadata,
  scanLitematics,
  searchBlocks,
} from "./api";
import SceneViewer from "./components/SceneViewer.vue";
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

const files = ref<SchematicSummary[]>([]);
const selected = ref<SchematicSummary | null>(null);
const analysis = ref<AnalysisResult | null>(null);
const analyzing = ref(false);
const saving = ref(false);
// 元数据编辑草稿。选中文件时用文件中的值填充
const draft = ref({ name: "", author: "", description: "" });
const dirty = ref(false);
const searchText = ref("");

/** 左侧目录栏宽度（像素）。
 *
 * 用户可以拖动中间的拖动条调整，因此不写死在样式里。
 * 取值同时受 MIN/MAX 约束：太窄会让「名称/制作版本/尺寸」三列挤在一起、
 * 太宽则把右侧预览与详情压得过小。
 */
const SIDEBAR_MIN_W = 320;
const SIDEBAR_MAX_W = 1200;
const sidebarWidth = ref(560);
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
    ? clampSidebar(560)
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
  onWindowResize();
});

// 组件卸载时摘掉 window 上的监听，避免拖动中关闭窗口而留下悬挂的监听
onBeforeUnmount(() => {
  window.removeEventListener("resize", onWindowResize);
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
    const res = await scanLitematics(cur.dirName, 1);
    const items = res.items.length > 0 ? res.items : files.value;
    files.value = items;
    // 转换会生成新文件并可能改动原文件，因此同步刷新选中项记录
    syncSelection(items);
  } catch (e) {
    Message.error(`转换失败：${e}`);
  } finally {
    convBusy.value = false;
  }
}

// 重复文件筛查
const dupOpen = ref(false);
const dupBusy = ref(false);
const dupResult = ref<DuplicateScan | null>(null);
const dupDir = ref("");

async function openDuplicates() {
  dupOpen.value = true;
  dupResult.value = null;
  // 默认扫描当前列表第一个文件所在目录，省去用户再选一次
  dupDir.value = selected.value?.dirName ?? files.value[0]?.dirName ?? "";
  if (!dupDir.value) return;
  await runDuplicateScan();
}

async function runDuplicateScan() {
  const dir = dupDir.value.trim();
  if (!dir) {
    Message.warning("请先填写要筛查的目录");
    return;
  }
  dupBusy.value = true;
  try {
    dupResult.value = await scanDuplicates(dir);
    const n = dupResult.value.groups.length;
    Message.success(n > 0 ? `发现 ${n} 组重复文件` : "没有发现重复文件");
  } catch (e) {
    Message.error(`筛查失败：${e}`);
    dupResult.value = null;
  } finally {
    dupBusy.value = false;
  }
}

/** 表格行键：组内路径拼接后唯一。
 *
 * 用于给表格数据补一个稳定且唯一的字段，再由 a-table 的 row-key 指向它。
 * Arco 的 row-key 只接受字符串（prop 类型是 StringConstructor），
 * 直接传函数不会报错但会被忽略，表格退化成按行号做键。
 */
function dupGroupKey(r: DuplicateGroup): string {
  return r.paths.join("|");
}

/** 供表格使用的重复组数据：在原对象上补一个 _key 字段 */
const dupGroupsWithKey = computed(() =>
  (dupResult.value?.groups ?? []).map((g) => ({ ...g, _key: dupGroupKey(g) })),
);

/** 一组重复文件可回收的空间：组内除保留一份外的其余副本之和。
 *
 * 大小必须取自 DuplicateGroup.size（去重阶段算出的单份大小），不能用
 * files.value 去反查：录制列表只含当前已扫描的那个目录，而重复筛查的
 * 目录由弹窗单独填写，两者常常不是同一个目录。反查失败时过去用 `?? 0`
 * 静默按 0 处理，于是「可回收」列恒显示 0 B，与同一行的「单个大小」矛盾。
 */
function wastedOf(size: number, paths: string[]): number {
  if (paths.length < 2) return 0;
  return size * (paths.length - 1);
}

const dupGroupColumns = [
  { title: "文件", slotName: "files" },
  { title: "单个大小", slotName: "size", width: 110 },
  { title: "可回收", slotName: "wasted", width: 110 },
];

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
      const res = await scanLitematics(cur.dirName, 1);
      const hit = res.items.find((f) => f.path === cur.path);
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

const columns: TableColumnData[] = [
  { title: "名称", dataIndex: "fileName", width: 240, ellipsis: true, tooltip: true },
  { title: "制作版本", dataIndex: "mcVersionName", width: 100 },
  { title: "尺寸", slotName: "dims", width: 130 },
  { title: "方块总数", dataIndex: "totalBlocks", width: 100 },
  { title: "来源目录", dataIndex: "dirName", ellipsis: true, tooltip: true },
];

const blockColumns: TableColumnData[] = [
  { title: "方块", dataIndex: "name", ellipsis: true, tooltip: true },
  { title: "状态", dataIndex: "state", ellipsis: true, tooltip: true },
  { title: "数量", dataIndex: "count", width: 90 },
  { title: "堆叠", slotName: "stacks", width: 110 },
];

const entityColumns: TableColumnData[] = [
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

async function pickDirectory() {
  // 对话框调用本身也可能失败（插件未就绪、系统对话框不可用）。
  // 放在 try 之外会让这个 Promise 静默 reject：用户点了「添加目录」
  // 却什么都没发生，也收不到任何提示。其余对话框调用都在 try 内，
  // 这里保持一致。
  let dir: unknown;
  try {
    dir = await open({ directory: true, multiple: false, title: "选择投影目录" });
  } catch (e) {
    Message.error(`打开目录选择框失败：${e}`);
    return;
  }
  if (typeof dir !== "string") return;
  status.value = "正在扫描…";
  try {
    const r = await scanLitematics(dir);
    files.value = r.items;
    syncSelection(r.items);
    const parts: string[] = [];
    const bad = r.items.filter((f) => f.error).length;
    if (bad > 0) parts.push(`${bad} 个文件解析失败`);
    // 被跳过的目录必须告知：否则结果看起来完整，用户会以为文件真的不存在
    if (r.skippedDirs > 0) {
      parts.push(`已跳过 ${r.skippedDirs} 个无法读取的目录`);
      // 列出被跳过的路径，否则用户无从判断少了哪些内容
      Message.warning({
        content: `以下目录无法读取，已跳过：\n${r.skippedPaths.join("\n")}`,
        duration: 8000,
      });
    }
    status.value = parts.length > 0 ? `扫描完成，${parts.join("，")}` : "扫描完成";
  } catch (e) {
    status.value = "扫描失败";
    Message.error(`扫描失败：${e}`);
  }
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
    const fresh = await scanLitematics(cur.dirName, 1);
    const hit = fresh.items.find((f) => f.path === cur.path);
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
    const r = await scanLitematics(cur.dirName, 1);
    const hit = r.items.find((f) => f.path === cur.path);
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

/** 导出当前视角为透明背景 PNG */
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

async function onRowClick(raw: TableData) {
  const record = raw as unknown as SchematicSummary;
  selected.value = record;
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
      <a-input-search
        v-model="searchText"
        size="small"
        placeholder="搜索名称/作者/文件名…"
        style="width: 240px"
        allow-clear
      />
      <a-button size="small" :disabled="!selected" @click="openReplace">方块替换</a-button>
      <a-button size="small" :disabled="!selected" @click="openConvert">版本转换</a-button>
      <a-button size="small" @click="openDuplicates">重复文件筛查</a-button>
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
        <a-table
          :columns="columns"
          :data="filteredFiles"
          :pagination="false"
          row-key="path"
          size="small"
          :bordered="{ wrapper: true, cell: false }"
          :scroll="{ y: '100%' }"
          @row-click="onRowClick"
        >
          <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
          <template #empty>
            <a-empty description="点击「添加目录」选择投影所在文件夹" />
          </template>
        </a-table>
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

      <div class="detail">
        <div class="preview">
          <div class="preview__stage">
            <a-spin :loading="sceneLoading" style="width: 100%; height: 100%">
              <SceneViewer
                ref="viewer"
                :scene="scene"
                :brightness="brightness"
              />
            </a-spin>
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
            <a-select v-model="exportSize" size="small" style="width: 104px">
              <a-option :value="512">512</a-option>
              <a-option :value="1024">1024</a-option>
              <a-option :value="2048">2048</a-option>
              <a-option :value="3072">3072</a-option>
            </a-select>
            <a-button
              size="small"
              type="primary"
              :loading="exporting"
              :disabled="!scene"
              @click="exportPreview"
            >
              导出预览图
            </a-button>
            <span class="preview__spacer" />
            <span v-if="sceneError" class="preview__error">{{ sceneError }}</span>
            <span v-else-if="sceneInfo" class="preview__info">{{ sceneInfo }}</span>
          </div>
        </div>
        <a-tabs default-active-key="info" style="padding: 0 8px">
          <a-tab-pane key="info" title="信息">
            <template v-if="selected">
              <a-form :model="draft" layout="vertical" size="small">
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
                <a-form-item label="描述">
                  <a-textarea
                    v-model="draft.description"
                    :auto-size="{ minRows: 3, maxRows: 8 }"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-space>
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
                </a-space>
              </a-form>
              <a-divider style="margin: 12px 0" />
            </template>
            <a-descriptions
              v-if="selected"
              :column="1"
              size="small"
              :label-style="{ width: '110px' }"
              bordered
            >
              <a-descriptions-item label="文件名">{{ selected.fileName }}</a-descriptions-item>
              <a-descriptions-item label="制作版本">
                {{ selected.mcVersionName || "未知" }}（数据版本 {{ selected.dataVersion }}）
              </a-descriptions-item>
              <a-descriptions-item label="投影格式">v{{ selected.formatVersion }}</a-descriptions-item>
              <a-descriptions-item label="尺寸(X×Y×Z)">
                {{ formatDims(selected.enclosing) }}
              </a-descriptions-item>
              <a-descriptions-item label="区域数量">{{ selected.regionCount }}</a-descriptions-item>
              <a-descriptions-item label="方块总数">{{ selected.totalBlocks }}</a-descriptions-item>
              <a-descriptions-item label="实体数量">{{ selected.entityCount }}</a-descriptions-item>
              <a-descriptions-item label="方块实体">{{ selected.tileEntityCount }}</a-descriptions-item>
              <a-descriptions-item label="待处理刻">{{ selected.pendingTicks }}</a-descriptions-item>
              <a-descriptions-item label="内含预览图">
                {{ selected.hasPreview ? "是" : "否" }}
              </a-descriptions-item>
              <a-descriptions-item label="文件大小">{{ formatSize(selected.fileSize) }}</a-descriptions-item>
              <a-descriptions-item v-if="selected.error" label="解析错误">
                <span style="color: rgb(var(--danger-6))">{{ selected.error }}</span>
              </a-descriptions-item>
            </a-descriptions>
            <a-empty v-else description="选择一个投影文件以查看详情" />
          </a-tab-pane>

          <a-tab-pane key="blocks" title="方块">
            <a-spin :loading="analyzing" style="width: 100%">
              <template v-if="analysis">
                <a-space style="margin-bottom: 8px" :size="16">
                  <span>非空气方块：<b>{{ analysis.nonAirTotal }}</b></span>
                  <span>空气方块：<b>{{ analysis.airTotal }}</b></span>
                  <span>方块种类：<b>{{ analysis.distinctBlockTypes }}</b></span>
                  <span>状态种类：<b>{{ analysis.distinctStates }}</b></span>
                </a-space>
                <div class="hint-dim" style="margin-bottom: 6px">
                  点击任意行可以把它设为方块替换的源方块
                </div>
                <a-table
                  :columns="blockColumns"
                  :data="analysis.blocks"
                  :pagination="{ pageSize: 20 }"
                  row-key="state"
                  size="small"
                  @row-click="onBlockRowClick"
                  :scroll="{ y: 360 }"
                >
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
              />
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
        <a-button size="small" :loading="packBusy" @click="refreshPacks">刷新</a-button>
      </a-space>

      <div class="hint-dim" style="margin-bottom: 10px">
        选具体的 .jar 文件而不是文件夹：
        <code>.minecraft\versions\&lt;版本&gt;\&lt;版本&gt;.jar</code>
        。要选客户端而不是服务端——服务端没有贴图资源。
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
      v-model:visible="dupOpen"
      title="重复文件筛查"
      :width="720"
      :footer="false"
      unmount-on-close
    >
      <a-space style="margin-bottom: 10px; width: 100%">
        <a-input v-model="dupDir" placeholder="要筛查的目录" allow-clear style="width: 420px" />
        <a-button size="small" :loading="dupBusy" @click="runDuplicateScan">开始筛查</a-button>
      </a-space>
      <div class="hint-dim" style="margin-bottom: 10px">
        只筛查该目录本身、不递归子目录；判定依据是文件内容，与文件名无关。
      </div>
      <a-spin :loading="dupBusy" style="width: 100%">
        <template v-if="dupResult">
          <a-alert v-if="dupResult.groups.length === 0" type="success">
            没有发现内容相同的文件
          </a-alert>
          <a-table
            v-else
            :columns="dupGroupColumns"
            :data="dupGroupsWithKey"
            :pagination="{ pageSize: 8 }"
            size="small"
            row-key="_key"
          >
            <template #files="{ record }">
              <div v-for="p in record.paths" :key="p" class="dup-path">{{ p }}</div>
            </template>
            <template #size="{ record }">{{ formatSize(record.size) }}</template>
            <template #wasted="{ record }">{{ formatSize(wastedOf(record.size, record.paths)) }}</template>
          </a-table>

          <a-alert
            v-if="dupResult.unreadable.length > 0"
            type="warning"
            style="margin-top: 12px"
          >
            <template #title>
              有 {{ dupResult.unreadable.length }} 个文件无法读取，未参与判定
            </template>
            <div v-for="u in dupResult.unreadable" :key="u.path" class="dup-path">
              {{ u.path }} —— {{ u.reason }}
            </div>
          </a-alert>
        </template>
        <a-empty v-else description="尚未筛查" />
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
