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
import type { BlockHit, BlockRow, ReplaceRule, SchematicSummary, VersionOption } from "../types";

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
  const blockVersions = ref<VersionOption[]>([]);
  const replaceForm = ref({
    fromName: "",
    fromPropsText: "",
    toName: "",
    toPropsText: "",
    inheritProps: true,
  });
  const replacePreview = ref<number | null>(null);

  // 右侧方块列表：搜索并点选
  const blockPickQuery = ref("");
  const blockHits = ref<BlockHit[]>([]);
  const blockSearching = ref(false);
  /** 搜索请求序号：只接受最后一次查询的结果（逐字符搜会并发多个请求） */
  let blockSearchSeq = 0;

  /** 右侧列表点选后填到哪一个输入框。
   *
   * 弹窗里有两个方块输入框（源、目标），右侧列表必须知道该填哪个，
   * 否则点下去不知道改的是哪一栏。跟随用户最后点过/聚焦的那个输入框，
   * 默认目标是替换目标——替换时更常选的是「换成什么」。
   */
  const replacePickField = ref<"from" | "to">("to");

  /** 方块 ID -> 中文名。
   *
   * 源方块与目标方块共用一份：两个输入框都要显示中文名，而来源有三处
   * （手输、右侧列表点选、从「方块」页签点行填入），汇总到一处才不会
   * 出现同一个方块在两个框里显示不同名字。
   */
  const blockZh = ref<Record<string, string>>({});

  /** 已经查过的方块 ID。
   *
   * 与 blockZh 分开是为了区分「还没查」与「库里没有这个名字」：
   * 只看有没有中文名的话，两种情况的界面表现一样，用户分不清是自己
   * 还没查还是这个方块本来就没有中文名。
   */
  const blockZhChecked = ref<Set<string>>(new Set());

  /** 待触发的查询，按方块 ID 分开存。
   *
   * 不能共用一个计时器：在两个输入框之间快速输入时，后一次输入会把前一个
   * 还没落地的查询取消掉，那一栏的中文名就再也不显示了。
   */
  const blockZhTimers = new Map<string, ReturnType<typeof setTimeout>>();

  /** 从「方块 ID 或 带属性的状态串」里取出纯 ID。
   *
   * 输入框是自由文本，用户可能把手输/粘贴的 `minecraft:chest[facing=east]`
   * 这种状态串直接留在框里。严格按整串去方块库比对必然匹配不上，于是明明
   * 能找到的方块也会被报成「未找到」。方块库里的名字不含属性，因此先剥掉
   * 方括号部分。
   */
  function bareBlockId(raw: string): string {
    const i = raw.indexOf("[");
    return (i >= 0 ? raw.slice(0, i) : raw).trim();
  }

  /** 把「方块名[属性]」拆成方块名与属性串；不是这种写法时返回 null。
   *
   * 后端契约要求方块名不含属性（见 replace.rs 的 ReplaceRule）：带属性的名字
   * 会被原样写进投影的调色板，游戏读不出来。旧界面用下拉框选方块，取值域
   * 天然挡住了这条路；改成自由文本后必须自己拆。拆法与从「方块」页签点行
   * 填入一致——名字进方块栏，方括号里的进属性栏，用户看得见拆成了什么。
   */
  function splitStateString(raw: string): { name: string; props: string } | null {
    const m = /^([^[\]]+)\[([^\]]*)\]\s*$/.exec(raw.trim());
    if (!m) return null;
    return { name: m[1].trim(), props: m[2].trim() };
  }

  function rememberBlockZh(id: string, zh: string) {
    const checked = new Set(blockZhChecked.value);
    checked.add(id);
    blockZh.value = { ...blockZh.value, [id]: zh };
    blockZhChecked.value = checked;
  }

  /** 查一个方块的中文名。查不到时记为空串（界面据此说明库里没有） */
  async function lookupBlockZh(raw: string) {
    const id = bareBlockId(raw);
    if (!id || blockZhQueried(id)) return;
    try {
      const hits = await searchBlocks(id, 50);
      // 只认名字完全一致的命中：搜索是模糊匹配，取自第一项会把
      // minecraft:oak_stairs 显示成 minecraft:oak_planks 这类近邻的中文名
      const exact = hits.find((h) => h.name === id);
      rememberBlockZh(id, exact ? exact.zh || exact.en || "" : "");
    } catch (e) {
      // 查不到名字不该打断替换流程，但也不能悄悄当成「库里没有」：
      // 记为未查过，下次改动会再试一遍
      Message.error(`查询方块中文名失败：${e}`);
    }
  }

  /** 输入框每敲一个字符都会触发，用防抖；逐字符查库既无必要也拖慢输入 */
  function scheduleBlockZh(raw: string) {
    const id = bareBlockId(raw);
    if (!id) return;
    const pending = blockZhTimers.get(id);
    if (pending !== undefined) clearTimeout(pending);
    blockZhTimers.set(
      id,
      setTimeout(() => {
        blockZhTimers.delete(id);
        void lookupBlockZh(id);
      }, 200),
    );
  }

  /** 方块中文名；没有记录时返回空串 */
  function blockZhOf(raw: string): string {
    const id = bareBlockId(raw);
    return id ? blockZh.value[id] ?? "" : "";
  }

  /** 这个方块是否已经查过名字（用于区分「没查」与「库里没有」） */
  function blockZhQueried(raw: string): boolean {
    return blockZhChecked.value.has(bareBlockId(raw));
  }

  /** 让右侧列表接下来填到指定的输入框 */
  function setPickField(field: "from" | "to") {
    replacePickField.value = field;
  }

  /** 方块栏的值变了：先拆掉可能粘贴进来的属性串，再查中文名。
   *
   * 拆完会再触发一次本函数（值已不含方括号，第二次直接走到查询），因此
   * 不会反复。
   */
  function onNameChanged(raw: string, field: "from" | "to") {
    const split = splitStateString(raw);
    if (split) {
      if (field === "from") {
        replaceForm.value.fromName = split.name;
        replaceForm.value.fromPropsText = split.props;
      } else {
        replaceForm.value.toName = split.name;
        replaceForm.value.toPropsText = split.props;
      }
      return;
    }
    scheduleBlockZh(raw);
  }

  watch(() => replaceForm.value.fromName, (v) => onNameChanged(v, "from"));
  watch(() => replaceForm.value.toName, (v) => onNameChanged(v, "to"));

  /** 预览请求序号：规则改过之后，早先发出去的预览结果不能再写回界面 */
  let previewSeq = 0;

  // 规则一改，之前算出的替换数量就不再对应当前规则了：留着那个数字会让
  // 用户以为「按现在填的规则只有这么多个」。同时在途的预览请求也一并作废。
  watch(
    replaceForm,
    () => {
      replacePreview.value = null;
      previewSeq += 1;
    },
    { deep: true },
  );

  // 卸载时清掉待触发的查询：否则组件已销毁仍会发一次无意义的请求
  onBeforeUnmount(() => {
    for (const t of blockZhTimers.values()) clearTimeout(t);
    blockZhTimers.clear();
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
      // 再剥一次属性：契约要求方块名不含属性，而这里是最后一次把关，
      // 带属性的名字会被原样写进投影的调色板
      fromName: bareBlockId(f.fromName),
      fromProps: parseProps(f.fromPropsText),
      toName: bareBlockId(f.toName),
      toProps: parseProps(f.toPropsText),
      inheritProps: f.inheritProps,
    };
  }

  async function onBlockSearch(text: string) {
    const q = text.trim();
    // 记下这次查询的序号：逐字符搜索会并发多个请求，慢的旧响应回来时
    // 不能覆盖新结果，否则列表内容与输入框里的关键字对不上
    const seq = ++blockSearchSeq;
    if (!q) {
      blockHits.value = [];
      // 空查询自己就是最新的一次查询，而它在途的前一次查询已经因为序号
      // 不匹配不再复位这个标志；这里必须复位，否则「输入后立刻清空」会把
      // 转圈状态永久留在界面上
      blockSearching.value = false;
      return;
    }
    blockSearching.value = true;
    try {
      const hits = await searchBlocks(q, 200);
      if (seq !== blockSearchSeq) return; // 已有更新的查询，丢弃这份过期结果
      blockHits.value = hits;
      // 顺手把结果里的中文名记下来（并标记为已查过），点选时就不必再查
      const zh = { ...blockZh.value };
      const checked = new Set(blockZhChecked.value);
      for (const h of hits) {
        zh[h.name] = h.zh || h.en || "";
        checked.add(h.name);
      }
      blockZh.value = zh;
      blockZhChecked.value = checked;
    } catch (e) {
      if (seq !== blockSearchSeq) return;
      Message.error(`检索方块失败：${e}`);
    } finally {
      if (seq === blockSearchSeq) blockSearching.value = false;
    }
  }

  /** 点选右侧列表里的方块，填到当前选定的那个输入框 */
  function pickBlock(hit: BlockHit) {
    const f = replaceForm.value;
    rememberBlockZh(hit.name, hit.zh || hit.en || "");
    if (replacePickField.value === "from") {
      // 换源方块必须同时清掉属性限定：属性是上一个方块特有的，留着它
      // 会让这条规则一个都匹配不上，而界面看上去「明明填好了」
      const hadProps = f.fromPropsText.trim().length > 0;
      f.fromName = hit.name;
      f.fromPropsText = "";
      Message.success(
        `已设为替换源：${hit.zh || hit.name}` + (hadProps ? "（属性限定已清空）" : ""),
      );
    } else {
      // 目标属性覆盖同理：它是无条件写上去的（replace.rs 里 to_props 最后
      // 强制 set_prop），留着上一个目标方块的属性会把新目标也带上那个属性
      const hadProps = f.toPropsText.trim().length > 0;
      f.toName = hit.name;
      f.toPropsText = "";
      Message.success(
        `已设为替换目标：${hit.zh || hit.name}` +
          (hadProps ? "（目标属性覆盖已清空）" : ""),
      );
    }
  }

  /** 点方块统计表的一行，把它设为替换源。
   *
   * 表里的行按**物品**合计，可能跨多个方块名（火把与墙上的火把），而替换规则
   * 在后端是按方块名精确相等匹配的（见 replace.rs 的 matches）。因此：
   *
   *   - 只有该行只对应一个方块名、且只有一条状态记录时，才把状态串里的属性
   *     填进源属性限定；
   *   - 跨了多个方块名时必须说清只覆盖了哪一个，否则用户会以为「替换整种物品」
   *     而实际上另一个形态原样不动——这种「看不出来的漏替换」比多替换更难发现。
   */
  function onBlockRowClick(raw: TableData) {
    const rec = raw as unknown as BlockRow;
    activeBlock.value = { name: rec.name };
    replaceForm.value.fromName = rec.name;
    // 状态串形如 minecraft:oak_stairs[facing=east,half=top]，
    // 方括号内的部分正好可以直接作为源属性限定
    const single = rec.forms.length === 1 && rec.stateCount === 1;
    const m = single ? /\[(.+)\]$/.exec(rec.state ?? "") : null;
    replaceForm.value.fromPropsText = m ? m[1] : "";
    if (rec.forms.length > 1) {
      const others = rec.forms.filter((f) => f !== rec.name);
      Message.warning(
        `已设为替换源：${rec.name}。这一行合计了 ${rec.forms.length} 种形态，` +
          `本条规则只替换 ${rec.name}，不包括 ${others.join("、")}`,
      );
      return;
    }
    Message.info(
      `已设为替换源：${rec.name}` + (single ? "" : "（不限属性，这一行合计了多个状态）"),
    );
  }

  async function openReplace() {
    replaceOpen.value = true;
    replacePreview.value = null;
    blockPickQuery.value = "";
    blockHits.value = [];
    // 列表默认填「替换目标」：替换时更常选的是「换成什么」，
    // 而源方块多半是从「方块」页签点行带过来的
    replacePickField.value = "to";
    // 中文名缓存每次打开都作废：方块库可能在本程序运行期间被换掉
    // （导入资源包后后端每条命令都会重新读库），留着旧结果会让明明能查到的
    // 方块一直显示「未找到」。顺带清掉还没落地的查询，免得它们写进刚清空的表。
    for (const t of blockZhTimers.values()) clearTimeout(t);
    blockZhTimers.clear();
    blockZh.value = {};
    blockZhChecked.value = new Set();
    // 源方块默认取当前选中的方块，省去用户重打一遍
    if (!replaceForm.value.fromName && activeBlock.value) {
      replaceForm.value.fromName = activeBlock.value.name;
    }
    // 本次打开时就查一次中文名：值可能来自上次的填写而没有发生变化，
    // 只靠 watch 不会触发，那一栏就会空着
    void lookupBlockZh(replaceForm.value.fromName);
    void lookupBlockZh(replaceForm.value.toName);
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
    // 记下这次预览的序号：等待期间用户可能改了规则（那时结果已经不作数），
    // 也可能连点了两次，早先的结果不能盖住后来的
    const seq = ++previewSeq;
    replaceBusy.value = true;
    try {
      const r = await previewReplace(cur.path, rule);
      if (seq !== previewSeq) return;
      replacePreview.value = r.replaced;
    } catch (e) {
      if (seq !== previewSeq) return;
      Message.error(`预览失败：${e}`);
    } finally {
      if (seq === previewSeq) replaceBusy.value = false;
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
    blockPickQuery,
    blockHits,
    blockSearching,
    blockVersions,
    replacePickField,
    replaceForm,
    replacePreview,
    blockZhOf,
    blockZhQueried,
    onBlockSearch,
    onBlockRowClick,
    setPickField,
    pickBlock,
    openReplace,
    doPreview,
    doReplace,
  };
}

export type BlockReplaceSlice = ReturnType<typeof createBlockReplaceSlice>;
