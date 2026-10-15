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
import { ref } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import {
  analyzeConversion,
  applyConversion,
  convertTargets,
} from "../api";
import type { ConvertItem, ConvertTarget, SchematicSummary } from "../types";

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
    if (!s.selected.value) return;
    try {
      convTargets.value = await convertTargets(s.selected.value.path);
    } catch (e) {
      Message.error(`读取可转换版本失败：${e}`);
      convTargets.value = [];
    }
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
    openConvert,
    runConvertAnalyze,
    doConvert,
  };
}

export type VersionConvertSlice = ReturnType<typeof createVersionConvertSlice>;
