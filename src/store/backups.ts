/**
 * 备份与操作日志域：备份抽屉的清单、还原与删除，以及全局操作日志。
 *
 * 备份只跟「当前选中的投影」有关，日志是全局的。两者放成一个域，
 * 一是共用同一组刷新动作，二是日志同时供备份抽屉的页签和独立的
 * 「日志管理」窗口使用（见 components/LogModal.vue），两边共用同一份
 * logs 状态就不会出现一个清空了、另一个还显示旧内容。
 *
 * 与其他域的协作：还原成功后经 BackupsDeps 让文件列表域重扫、让 3D
 * 预览域重建场景，保证界面显示的是还原后的真实内容。
 * 依赖只声明本域用到的成员，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  clearOperationLog,
  deleteBackup,
  listBackups,
  openOperationLogDir,
  readOperationLog,
  restoreBackup,
} from "../api";
import type { BackupEntry, SchematicSummary } from "../types";

/** 备份与日志域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface BackupsDeps {
  /** 文件列表域的当前选中；备份清单与还原的作用对象 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的文件清单；还原后按路径找回同名文件 */
  files: ComputedRef<SchematicSummary[]>;
  /** 文件列表域的重扫；还原覆盖文件后刷新清单 */
  rescanAll: () => Promise<void>;
  /** 文件列表域的草稿复位；还原后重新载入元数据 */
  resetDraft: (rec: SchematicSummary) => void;
  /** 3D 预览域的场景加载；还原后重建场景 */
  loadScene: (record: SchematicSummary) => void;
}

/** 一次读取的日志条数上限。
 *
 * 「日志管理」窗口里的说明文案引用它，避免同一个数字在界面与这里各写一遍。
 */
const LOG_MAX_LINES = 300;

export function createBackupsSlice(s: BackupsDeps) {
  // 备份与日志
  const backupOpen = ref(false);
  const backups = ref<BackupEntry[]>([]);
  const backupLoading = ref(false);
  const logs = ref<string[]>([]);
  const logLoading = ref(false);
  /** 独立的「日志管理」窗口。与备份抽屉里的日志页签看的是同一份内容，
   *  入口在备份与日志的「操作日志」页签里。 */
  const logOpen = ref(false);

  function formatTime(ns: string): string {
    if (!ns) return "—";
    // 非纯数字直接视为无法解析，而不是靠 BigInt 抛错再走 catch：
    // 把异常当正常分支用会掩盖真正的解析错误
    if (!/^\d+$/.test(ns)) return "—";
    // 纳秒用 BigInt 解析再转毫秒，避免超出安全整数范围导致时间错乱
    const ms = Number(BigInt(ns) / 1000000n);
    const d = new Date(ms);
    return Number.isNaN(d.getTime()) ? "—" : d.toLocaleString();
  }

  async function refreshBackups() {
    backupLoading.value = true;
    try {
      backups.value = await listBackups(s.selected.value?.fileName.replace(/\.litematic$/i, ""));
    } catch (e) {
      Message.error(`读取备份失败：${e}`);
    } finally {
      backupLoading.value = false;
    }
  }

  async function refreshLogs() {
    logLoading.value = true;
    try {
      logs.value = await readOperationLog(LOG_MAX_LINES);
    } catch (e) {
      Message.error(`读取日志失败：${e}`);
    } finally {
      logLoading.value = false;
    }
  }

  async function onRestore(row: BackupEntry) {
    const cur = s.selected.value;
    if (!cur) return;
    try {
      await restoreBackup(row.path, cur.path);
      Message.success("已还原");
      await Promise.all([refreshBackups(), refreshLogs()]);
      // 还原后重新读取文件内容与预览，避免界面仍显示还原前的数据
      await s.rescanAll();
      const hit = s.files.value.find((f) => f.path === cur.path);
      if (hit) {
        s.selected.value = hit;
        s.resetDraft(hit);
        const idx = s.files.value.findIndex((f) => f.path === hit.path);
        if (idx >= 0) s.files.value[idx] = hit;
        void s.loadScene(hit);
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

  async function openBackups() {
    backupOpen.value = true;
    await Promise.all([refreshBackups(), refreshLogs()]);
  }

  /** 打开「日志管理」窗口。打开即刷新：日志随时可能被其它操作写入，
   *  显示上一次打开时的旧内容会误导用户。 */
  async function openLogs() {
    logOpen.value = true;
    await refreshLogs();
  }

  /** 清空操作日志。清空是有破坏性的，二次确认由界面负责。 */
  async function clearLogs() {
    try {
      await clearOperationLog();
      Message.success("已清空操作日志");
      await refreshLogs();
    } catch (e) {
      Message.error(`清空日志失败：${e}`);
    }
  }

  /** 在系统文件管理器里打开日志所在目录 */
  async function openLogDir() {
    try {
      await openOperationLogDir();
    } catch (e) {
      Message.error(`打开日志目录失败：${e}`);
    }
  }

  return {
    backupOpen,
    backups,
    backupLoading,
    logs,
    logLoading,
    logOpen,
    logMaxLines: LOG_MAX_LINES,
    formatTime,
    refreshBackups,
    refreshLogs,
    onRestore,
    onDelete,
    openBackups,
    openLogs,
    clearLogs,
    openLogDir,
  };
}

export type BackupsSlice = ReturnType<typeof createBackupsSlice>;
