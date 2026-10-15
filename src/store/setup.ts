/**
 * 版本库域：「版本库」弹窗的全部状态与动作——定位 .minecraft、
 * 扫描已装版本、勾选缺失版本并下载/构建基础库。
 *
 * 基础库是方块检索与 3D 预览的数据底座：库里一个版本都没有时，
 * 这些功能都无数据可显示，所以启动时发现库为空会自动弹出本界面
 * 引导补齐；之后也可从工具栏再次打开补充缺失版本。
 *
 * 与其他域的协作：不直接读写其他域的状态；补库完成后后端的库缓存
 * 已失效，各域下次取数据（listBlockVersions、buildScene 等）自然
 * 用上新库，无需在这里逐个通知。
 */
import { computed, onMounted, ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  baseDbStatus,
  defaultMinecraftDir,
  loadSetupState,
  onBaseSetupProgress,
  saveSetupState,
  scanMinecraft,
  setupAddVersions,
} from "../api";
import type {
  BaseSetupProgress,
  BaseVersion,
  McVersion,
  SetupSource,
  SetupVersionOutcome,
} from "../types";

/** 会被记住的「版本库」界面状态。版本号用于日后改结构时丢弃旧数据。 */
interface SetupState {
  version: 1;
  mcDir: string;
  source: SetupSource;
}

const SETUP_STATE_VERSION = 1;

/** 一个支持版本在界面上的状态 */
export interface SetupRow {
  name: string;
  dataVersion: number;
  /** ready=基础库已有；local=本机已装可直接用；missing=需要下载 */
  state: "ready" | "local" | "missing";
  jarPath: string | null;
}

export function createSetupSlice() {
  const setupOpen = ref(false);
  /** 打开弹窗过程中的加载态，供工具栏按钮显示 */
  const setupOnOpen = ref(false);
  const scanning = ref(false);
  const running = ref(false);

  const mcDir = ref("");
  const source = ref<SetupSource>("official");

  /** 后端支持表与就绪清单；行状态由它们和扫描结果合成 */
  const supported = ref<BaseVersion[]>([]);
  const ready = ref<BaseVersion[]>([]);
  const entityAvailable = ref(false);

  /** 本机已装版本的扫描结果（id -> 命中）与读不出信息的目录说明 */
  const scanHits = ref<Map<string, McVersion>>(new Map());
  const scanWarnings = ref<string[]>([]);

  /** 勾选的待补版本；已就绪的行不可勾选 */
  const checked = ref<Set<string>>(new Set());

  /** 运行中的逐版本进度与上一批的结果，供弹窗展示 */
  const progress = ref<Map<string, BaseSetupProgress>>(new Map());
  const outcomes = ref<SetupVersionOutcome[]>([]);

  const rows = computed<SetupRow[]>(() =>
    supported.value.map((v) => {
      const hit = scanHits.value.get(v.name);
      if (ready.value.some((r) => r.name === v.name)) {
        // 就绪的版本也带上本机 jar：重做某一版本（贴图库缺了、包损坏了）
        // 时能用本地文件直接重建，不必再下一次 20 多 MB 的客户端
        return {
          name: v.name,
          dataVersion: v.dataVersion,
          state: "ready",
          jarPath: hit?.jarPath ?? null,
        };
      }
      return hit
        ? { name: v.name, dataVersion: v.dataVersion, state: "local", jarPath: hit.jarPath }
        : { name: v.name, dataVersion: v.dataVersion, state: "missing", jarPath: null };
    }),
  );

  const setupColumns = [
    { title: "", slotName: "check", width: 44 },
    { title: "版本", dataIndex: "name", width: 110 },
    { title: "数据版本", dataIndex: "dataVersion", width: 90 },
    { title: "状态", slotName: "state", width: 110 },
    { title: "进度 / 结果", slotName: "result" },
  ];

  /** 状态刷新后把勾选重置为「本机已装的版本」。
   *
   * 只自动勾这类：用本地 jar 构建，不花流量也不耗时，用户来这个界面
   * 通常就是想把已装的先补齐。清单里没有的版本要联网下载几十 MB，
   * 勾不勾必须由用户决定——默认全勾会让人一不小心点出一个 GB 级任务。
   */
  function resetChecked() {
    checked.value = new Set(
      rows.value.filter((r) => r.state === "local").map((r) => r.name),
    );
  }

  function toggleChecked(name: string, on: boolean) {
    const next = new Set(checked.value);
    if (on) next.add(name);
    else next.delete(name);
    checked.value = next;
  }

  async function refreshStatus() {
    const st = await baseDbStatus();
    supported.value = st.supported;
    ready.value = st.versions;
    entityAvailable.value = st.entityAvailable;
    resetChecked();
  }

  /** 把本机已装的版本并进勾选，保留用户已经手动勾上的。
   *
   * 扫描结果比状态查询晚到（列表要先出来），不补这一步的话，
   * 扫出的本地版本不会出现在默认勾选里。
   */
  function addLocalToChecked() {
    const next = new Set(checked.value);
    for (const r of rows.value) {
      if (r.state === "local") next.add(r.name);
    }
    checked.value = next;
  }

  /** 记住目录与来源选择；失败只留日志，不该打断界面操作 */
  async function persistSetupState() {
    const st: SetupState = {
      version: SETUP_STATE_VERSION,
      mcDir: mcDir.value,
      source: source.value,
    };
    try {
      await saveSetupState(JSON.stringify(st));
    } catch (e) {
      console.warn("保存版本库状态失败：", e);
    }
  }

  /** 恢复上次的目录与来源；没有记录时退回官方启动器的默认路径 */
  async function restoreSetupState() {
    let raw: string | null = null;
    try {
      raw = await loadSetupState();
    } catch (e) {
      console.warn("读取版本库状态失败：", e);
    }
    if (raw) {
      try {
        const st = JSON.parse(raw) as Partial<SetupState>;
        if (st.version === SETUP_STATE_VERSION) {
          if (typeof st.mcDir === "string") mcDir.value = st.mcDir;
          if (st.source === "official" || st.source === "bmcl") source.value = st.source;
        }
      } catch (e) {
        console.warn("版本库状态内容无法解析，已忽略：", e);
      }
    }
    if (mcDir.value) return;
    try {
      const def = await defaultMinecraftDir();
      if (def) mcDir.value = def;
    } catch (e) {
      console.warn("定位默认 .minecraft 路径失败：", e);
    }
  }

  /** 扫描目录里的已装版本 */
  async function rescan() {
    const dir = mcDir.value.trim();
    if (!dir) {
      Message.warning("请先填写或选择 .minecraft 目录");
      return;
    }
    scanning.value = true;
    try {
      const r = await scanMinecraft(dir);
      scanHits.value = new Map(r.versions.map((v) => [v.id, v]));
      scanWarnings.value = r.warnings;
      addLocalToChecked();
      await persistSetupState();
    } catch (e) {
      Message.error(`扫描失败：${e}`);
    } finally {
      scanning.value = false;
    }
  }

  /** 手动选择 .minecraft 目录 */
  async function pickMcDir() {
    const dir = await open({ directory: true, multiple: false, title: "选择 .minecraft 目录" });
    if (typeof dir !== "string" || !dir) return;
    mcDir.value = dir;
    await rescan();
  }

  async function selectSource(v: SetupSource) {
    source.value = v;
    await persistSetupState();
  }

  /** 打开弹窗：恢复状态、刷新基础库状态，目录已知时顺手扫描 */
  async function openSetup() {
    if (setupOpen.value) return;
    setupOpen.value = true;
    setupOnOpen.value = true;
    try {
      await restoreSetupState();
      await refreshStatus();
      if (mcDir.value) await rescan();
    } catch (e) {
      Message.error(`读取版本库状态失败：${e}`);
    } finally {
      setupOnOpen.value = false;
    }
  }

  /** 开始准备勾选的版本：下载/使用本地 jar 并构建基础库 */
  async function startSetup() {
    if (running.value) return;
    const targets = rows.value.filter((r) => checked.value.has(r.name));
    if (targets.length === 0) {
      Message.warning("没有勾选任何待补版本");
      return;
    }
    running.value = true;
    outcomes.value = [];
    progress.value = new Map();
    // 订阅进度事件；结束后退订。listen 失败（窗口环境异常）只影响进度
    // 展示，不影响补库本身，因此退了订也要如实留日志
    let unlisten: (() => void) | null = null;
    try {
      unlisten = await onBaseSetupProgress((p) => {
        const next = new Map(progress.value);
        next.set(p.id, p);
        progress.value = next;
      });
    } catch (e) {
      console.warn("订阅版本库进度事件失败：", e);
    }
    try {
      const r = await setupAddVersions(
        source.value,
        targets.map((t) => ({ id: t.name, dataVersion: t.dataVersion, jarPath: t.jarPath })),
      );
      outcomes.value = r.outcomes;
      const failed = r.outcomes.filter((o) => !o.ok);
      if (failed.length > 0) {
        Message.warning({
          content: `${r.outcomes.length - failed.length} 个版本已就绪，${failed.length} 个失败（详见弹窗列表）`,
          duration: 8000,
        });
      } else {
        Message.success(`${r.outcomes.length} 个版本已全部就绪`);
      }
      await refreshStatus();
    } catch (e) {
      Message.error(`补库任务失败：${e}`);
    } finally {
      unlisten?.();
      running.value = false;
    }
  }

  onMounted(() => {
    // 启动时检查基础库：一个版本都没有就意味着方块检索和 3D 预览
    // 都没有数据可显示，直接弹出引导界面比让用户点了一圈才发现
    // 各处空白更友好
    void (async () => {
      try {
        const st = await baseDbStatus();
        supported.value = st.supported;
        ready.value = st.versions;
        entityAvailable.value = st.entityAvailable;
        if (st.versions.length === 0) await openSetup();
      } catch (e) {
        console.warn("启动检查版本库状态失败：", e);
      }
    })();
  });

  return {
    setupOpen,
    setupOnOpen,
    scanning,
    running,
    mcDir,
    source,
    supported,
    ready,
    entityAvailable,
    scanWarnings,
    checked,
    progress,
    outcomes,
    rows,
    setupColumns,
    toggleChecked,
    rescan,
    pickMcDir,
    selectSource,
    openSetup,
    startSetup,
  };
}

export type SetupSlice = ReturnType<typeof createSetupSlice>;
