/**
 * 资源包管理域：资源包弹窗的清单、导入（单个 jar / mods 文件夹）、
 * 启停与删除、叠加版本选择。
 *
 * 资源包是方块贴图与模型的来源，管理动作自成一组；清单刷新有并发
 * 计次，导入与开关有防重入，这些机制只服务于这个弹窗，不宜散到别处。
 *
 * 与其他域的协作：仅从方块库读版本列表（listBlockVersions），
 * 不直接改动其他域的状态。
 */
import { ref } from "vue";
import { Message } from "@arco-design/web-vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  deletePack,
  importJar,
  importModsFolder,
  listBlockVersions,
  listPacks,
  openDirectory,
  setPackEnabled,
} from "../api";
import type {
  ImportOutcome,
  ModsImport,
  PackEntry,
  PackLoadFailure,
  VersionOption,
} from "../types";
import { formatSize } from "../utils/format";

export function createPacksSlice() {
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
    // 置位必须在打开对话框**之前**：否则连点两次会开两个对话框并触发两次导入，
    // 两次批量导入交错读写清单，后写者会把先写者的记录覆盖掉（见 runImportJar）。
    modsBusy.value = true;
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择 mods 文件夹",
      });
      if (typeof picked !== "string") return;

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
      // 有意跳过的条目也要说：名字里含 client 的模组会被判成客户端包
      if (out.skipped.length > 0) {
        Message.info({
          content:
            `另有 ${out.skipped.length} 个文件未导入：` +
            out.skipped
              .slice(0, 3)
              .map((f) => `${f.path}（${f.reason}）`)
              .join("；") +
            (out.skipped.length > 3 ? ` …等 ${out.skipped.length} 个` : ""),
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

  return {
    packOpen,
    packBusy,
    packOnOpen,
    packs,
    packsDir,
    packSkipped,
    packLoadFailures,
    importKind,
    importDataVersion,
    importBusy,
    modsBusy,
    overlayVersions,
    packColumns,
    packPending,
    refreshPacks,
    openPacksDir,
    runImportJar,
    runImportModsFolder,
    togglePack,
    removePack,
    openPacksPanel,
  };
}

export type PacksSlice = ReturnType<typeof createPacksSlice>;
