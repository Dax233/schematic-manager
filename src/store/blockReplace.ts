/**
 * 方块替换域：替换弹窗的全部状态与流程（规则表单、方块检索、
 * 中文名联想、预览、执行替换）。
 *
 * 这是一个自包含的向导式流程：打开弹窗 → 填规则 → 预览 → 执行，
 * 状态只在弹窗生命周期内有意义，与文件列表、3D 预览彼此独立。
 *
 * 与其他域的协作：执行成功后经 BlockReplaceDeps 让文件列表域重扫并
 * 刷新选中项，再让 3D 预览域重建场景，使界面立刻反映替换结果。
 * 依赖只声明本域用到的成员，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { onBeforeUnmount, ref, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { TableData } from "@arco-design/web-vue";
import {
  applyReplace,
  listBlockVersions,
  previewReplace,
  searchBlocks,
} from "../api";
import type { BlockCount, BlockHit, ReplaceRule, SchematicSummary, VersionOption } from "../types";

/** 方块替换域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface BlockReplaceDeps {
  /** 文件列表域的当前选中；替换的作用对象 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的文件清单；替换后按路径找回同名文件 */
  files: ComputedRef<SchematicSummary[]>;
  /** 文件列表域的重扫；替换生成新文件后刷新清单 */
  rescanAll: () => Promise<void>;
  /** 文件列表域的草稿复位；替换后重新载入元数据 */
  resetDraft: (rec: SchematicSummary) => void;
  /** 3D 预览域的场景加载；替换后重建场景 */
  loadScene: (record: SchematicSummary) => void;
}

export function createBlockReplaceSlice(s: BlockReplaceDeps) {
  const activeBlock = ref<{ name: string } | null>(null);

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

  /** 源方块的中文名。
   *
   * 从方块库按 ID 反查。与目标方块的搜索共用同一个接口：源方块只差在
   * 用户是从「方块」页签点行填入而不是自己搜，但显示的仍是同一个方块库的
   * 中文名，两处的结果因此不会互相矛盾。
   */
  const fromZh = ref("");

  /** 源方块中文名的查询状态："idle" / "loading" / "done"。
   *
   * 没有这个状态时，输入的第一个字符到查询落地之间 fromZh 都是空的，
   * 界面会先显示「未找到该 ID」再改成正确的中文名——每敲一个字都先看到一句
   * 否定结论。只有确认查完了（done）才允许下结论。
   */
  const fromZhState = ref<"idle" | "loading" | "done">("idle");

  /** 请求序号：只接受最后一次查询的结果。
   *
   * 慢响应可能后到——打开弹窗时的即时查询与随后的防抖查询会并发，
   * 后到的旧结果会把新结果覆盖掉。
   */
  let fromZhSeq = 0;

  /** 从「方块 ID 或 带属性的状态串」里取出纯 ID。
   *
   * 用户从「方块」页签点行填入的是 `minecraft:chest[facing=east,type=...]`
   * 这种状态串，严格按整串去方块库比对必然匹配不上，于是明明能找到的方块
   * 也会被报成「未找到」。方块库里的名字不含属性，因此先剥掉方括号部分。
   */
  function bareBlockId(raw: string): string {
    const i = raw.indexOf("[");
    return (i >= 0 ? raw.slice(0, i) : raw).trim();
  }

  async function resolveFromZh() {
    const id = bareBlockId(replaceForm.value.fromName);
    const seq = ++fromZhSeq;
    if (!id) {
      fromZh.value = "";
      fromZhState.value = "idle";
      return;
    }
    fromZhState.value = "loading";
    try {
      const hits = await searchBlocks(id, 50);
      // 只认名字完全一致的命中：搜索是模糊匹配，取自第一项会把
      // minecraft:oak_stairs 显示成 minecraft:oak_planks 这类近邻的中文名
      const exact = hits.find((h) => h.name === id);
      if (seq !== fromZhSeq) return; // 已有更新的查询，丢弃这份过期结果
      fromZh.value = exact ? exact.zh || exact.en || "" : "";
      fromZhState.value = "done";
    } catch (e) {
      if (seq !== fromZhSeq) return;
      // 查不到中文名不该打断替换流程，但也不能假装查过了
      fromZh.value = "";
      fromZhState.value = "done";
      console.warn("查询源方块中文名失败：", e);
    }
  }

  // 用防抖：输入框每敲一个字符都会触发，逐字符查库既无必要也拖慢输入
  let fromZhTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleFromZh() {
    if (fromZhTimer !== null) clearTimeout(fromZhTimer);
    // 立刻切到「查询中」，避免防抖等待期间就显示「未找到」
    if (replaceForm.value.fromName.trim()) fromZhState.value = "loading";
    fromZhTimer = setTimeout(() => {
      fromZhTimer = null;
      void resolveFromZh();
    }, 200);
  }

  watch(() => replaceForm.value.fromName, scheduleFromZh);

  // 卸载时清掉待触发的查询：否则组件已销毁仍会发一次无意义的请求
  onBeforeUnmount(() => {
    if (fromZhTimer !== null) {
      clearTimeout(fromZhTimer);
      fromZhTimer = null;
    }
  });

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
    // 本次打开时就查一次中文名：值可能来自上次的填写而没有发生变化，
    // 只靠 watch 不会触发，那一栏就会空着
    fromZh.value = "";
    fromZhState.value = "idle";
    void resolveFromZh();
    if (blockVersions.value.length === 0) {
      try {
        blockVersions.value = await listBlockVersions();
      } catch (e) {
        Message.error(`读取版本列表失败：${e}`);
      }
    }
  }

  async function doPreview() {
    const cur = s.selected.value;
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
    const cur = s.selected.value;
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
        await s.rescanAll();
        const hit = s.files.value.find((f) => f.path === cur.path);
        if (hit) {
          s.selected.value = hit;
          s.resetDraft(hit);
          const idx = s.files.value.findIndex((f) => f.path === hit.path);
          if (idx >= 0) s.files.value[idx] = hit;
          void s.loadScene(hit);
        }
      }
    } catch (e) {
      Message.error(`替换失败：${e}`);
    } finally {
      replaceBusy.value = false;
    }
  }

  return {
    activeBlock,
    replaceOpen,
    replaceBusy,
    blockHits,
    blockSearching,
    blockVersions,
    replaceForm,
    replacePreview,
    fromZh,
    fromZhState,
    onBlockSearch,
    onBlockRowClick,
    openReplace,
    doPreview,
    doReplace,
  };
}

export type BlockReplaceSlice = ReturnType<typeof createBlockReplaceSlice>;
