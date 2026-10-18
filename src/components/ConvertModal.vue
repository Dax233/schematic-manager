/**
 * 版本转换弹窗。
 *
 * 向导式流程：选目标版本 → 分析差异 → 指定替代品 → 执行转换。
 * 状态与流程都在 store/versionConvert.ts，这里只做展示与触发。
 *
 * 布局分左右两栏：左栏是要处理的方块清单，右栏是方块列表（点选即指定）。
 * 用户必须同时看到「缺了哪些方块」与「有哪些方块可选」，挤在一个窄弹窗里
 * 就只能来回滚动，因此弹窗明显加宽、两栏并排。
 *
 * 凡是出现方块 ID 的地方都一并给出中文名：只给 ID 时用户无法确认自己
 * 要替换的到底是哪一个方块（中文名由后端随分析结果给出，见 dto.rs）。
 */
<script setup lang="ts">
import { computed, ref } from "vue";
import type { TableData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import { displayName } from "../i18n";
import { useAppStore } from "../store";
import type { ConvertItem } from "../types";

const {
  selected,
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
  convIsAuto,
  convReplState,
  convHitInTarget,
  convZhOf,
  convZhQueried,
  selectConvRow,
  onConvPickSearch,
  pickConvBlock,
  scheduleConvZhLookup,
  reloadConvTargets,
  runConvertAnalyze,
  doConvert,
  // 版本库域：已支持但尚未加载的版本也要列出来，点一下现场补齐
  supported,
  ready,
  progress,
  running,
  prepareVersion,
} = useAppStore();

/** 目标版本下拉的候选项。
 *
 * 已加载的版本（后端给了下标，可直接用来转换）排在最前；其余支持的版本
 * 以灰字列在后面，点一下就下载/构建，补完再选它。两者分开是必要的：
 * 只有加载过的版本有下标，没加载的连不上后端。
 */
interface TargetOption {
  /** 已加载版本在方块库里的下标；未加载的没有 */
  index: number | null;
  name: string;
  dataVersion: number;
  /** 是否已经加载，可以直接转换 */
  loaded: boolean;
  /** 该版本是否正在准备中（下载或构建） */
  preparing: boolean;
  /** 准备进度说明；未在准备时为空 */
  progressText: string;
}

/** 正在准备中的版本名。
 *
 * 补库过程是异步的，按钮上的转圈与「下载中 12%」都靠它；
 * 与 running 配合：整批补库在跑时不允许重复点。
 */
const preparingName = ref("");

const targetOptions = computed<TargetOption[]>(() => {
  const loaded: TargetOption[] = convTargets.value.map((t) => ({
    index: t.index,
    name: t.name,
    dataVersion: t.dataVersion,
    loaded: true,
    preparing: false,
    progressText: "",
  }));
  const loadedNames = new Set(loaded.map((o) => o.name));
  // 投影自己的版本与已加载的版本不重复列出：前者转过去没有意义，
  // 后者已经在上面那一组里
  const sourceVersion = selected.value?.dataVersion;
  const pending: TargetOption[] = supported.value
    .filter(
      (v) => !loadedNames.has(v.name) && !ready.value.some((r) => r.name === v.name),
    )
    .filter((v) => v.dataVersion !== sourceVersion)
    .map((v) => {
      const p = progress.value.get(v.name);
      return {
        index: null,
        name: v.name,
        dataVersion: v.dataVersion,
        loaded: false,
        preparing: preparingName.value === v.name,
        progressText: describeProgress(v.name, p),
      };
    });
  return [...loaded, ...pending];
});

/** 把进度事件的阶段与百分比说成人话 */
function describeProgress(
  name: string,
  p: { stage: string; received: number; total: number; error: string } | undefined,
): string {
  if (preparingName.value !== name || !p) return "";
  if (p.error) return `失败：${p.error}`;
  if (p.stage === "download") {
    const pct = p.total > 0 ? Math.floor((p.received / p.total) * 100) : 0;
    return `下载中 ${pct}%`;
  }
  return "正在构建";
}

/** 选中目标版本。
 *
 * 未加载的版本用一个 `pending:<版本名>` 前缀当值：选中它并不是「选定」，
 * 而是「开始准备这个版本」，因此这里不动已选中的目标版本，只发起补库。
 * 用下拉的 change 而不是给选项挂点击，是因为禁用项收不到点击事件，
 * 而禁用态又正是没下标可选的未加载版本该有的表现。
 */
function onTargetChange(v: unknown) {
  if (typeof v === "string" && v.startsWith("pending:")) {
    const opt = targetOptions.value.find((o) => `pending:${o.name}` === v);
    if (opt) void onPrepareVersion(opt);
    return;
  }
  convTargetIdx.value = typeof v === "number" ? v : undefined;
}

/** 点未加载的版本：现场下载/构建它，补完自动选中 */
async function onPrepareVersion(opt: TargetOption) {
  if (running.value || opt.loaded) return;
  preparingName.value = opt.name;
  try {
    const ok = await prepareVersion(opt.name);
    if (!ok) {
      Message.error(`${opt.name} 未能就绪，详见「版本库」里的结果`);
      return;
    }
    // 重新取一次列表：新版本在方块库里的下标只有后端知道
    await reloadConvTargets();
    const hit = convTargets.value.find((t) => t.name === opt.name);
    if (hit) {
      convTargetIdx.value = hit.index;
      Message.success(`${opt.name} 已就绪，已选为目标版本`);
    } else {
      Message.success(`${opt.name} 已就绪，可在「版本库」中查看`);
    }
  } finally {
    preparingName.value = "";
  }
}

/** 行只标「当前正在编辑」这一件事。
 *
 * 替代品的状态色不在这里染整行，也不染整格，而是只给那一格里的输入框上色
 * （见样式里的 .conv-block--todo / --auto）：整行或整格上色会把表格刷成
 * 一片绿红，连输入框下面的中文名与 ID 也泡在色块里，反而看不出重点。
 */
function convRowClass(raw: TableData): string {
  const rec = raw as unknown as ConvertItem;
  return convActiveItem.value?.name === rec.name ? "conv-row--active" : "";
}

function onConvRowClick(raw: TableData) {
  selectConvRow(raw as unknown as ConvertItem);
}

/** 替代品那一栏要显示的中文名。
 *
 * 三种情况必须分开：查到名字、查过但库里没有、还没查完。
 * 混在一起（都显示空白）时用户分不清是自己填错了 ID 还是界面没反应。 */
function replZh(rec: ConvertItem): string {
  const id = convReplacementOf(rec);
  if (!id) return "";
  const zh = convZhOf(id);
  if (zh) return zh;
  return convZhQueried(id) ? "方块库中没有这个名字" : "正在查询中文名…";
}
</script>

<template>
  <a-modal
    v-model:visible="convOpen"
    title="转换游戏版本"
    :width="1180"
    :ok-loading="convBusy"
    ok-text="开始转换"
    :ok-button-props="{ disabled: convTargetIdx === undefined || !selected || !convAnalyzed }"
    @ok="doConvert"
    modal-class="conv-window"
  >
    <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
      请先在左侧选择一个投影文件
    </a-alert>

    <a-space style="margin-bottom: 10px">
      <span>目标版本</span>
      <a-select
        :model-value="convTargetIdx"
        placeholder="选择要转换到的版本"
        allow-search
        style="width: 300px"
        @change="onTargetChange"
      >
        <a-option
          v-for="t in targetOptions"
          :key="t.name"
          :value="t.index ?? `pending:${t.name}`"
          :class="{ 'conv-version--pending': !t.loaded }"
        >
          <!-- 未加载的版本：选中它就等于「点一下开始准备」，由 change 处理；
               这里只负责把它画成灰字并标出进度 -->
          <span v-if="!t.loaded" class="conv-version__pending">
            <a-spin v-if="t.preparing" :size="12" />
            {{ t.name }}（dataVersion {{ t.dataVersion }}）
            <span class="conv-version__tag">{{ t.progressText || "未加载，点击下载" }}</span>
          </span>
          <span v-else>
            {{ t.name }}（dataVersion {{ t.dataVersion }}）
          </span>
        </a-option>
      </a-select>
      <a-button size="small" :loading="convBusy" @click="runConvertAnalyze">分析差异</a-button>
    </a-space>

    <div class="hint-dim" style="margin-bottom: 10px">
      转换结果另存为新文件，原文件不会被改动。目标版本里已不存在的方块需要
      指定替代品：在左侧点一行选中它，再到右侧方块列表里点一个方块即可
      （也可以在「替代为」里直接填方块 ID）。
      留着空白的方块转换后保持原样。
    </div>

    <a-spin :loading="convBusy" style="width: 100%">
      <template v-if="convAnalyzed">
        <a-alert v-if="convItems.length === 0" type="success">
          该投影用到的方块在目标版本中都存在，可以直接转换
        </a-alert>

        <div v-else class="conv-split">
          <!-- 左栏：缺哪些方块。红=还没定下替代品，绿=已经有了（自动推荐或已指定） -->
          <div class="conv-split__pane conv-split__pane--main">
            <a-table
              :columns="convColumns"
              :data="convItems"
              :pagination="{ pageSize: 12 }"
              :row-class="convRowClass"
              row-key="name"
              size="small"
              @row-click="onConvRowClick"
            >
              <template #from="{ record }">
                <div class="conv-block">
                  <!-- 中文名由后端随分析结果给出，正常不会有空值（查不到时
                       退回去命名空间的方块名），因此不再写「没有这个名字」的兜底 -->
                  <span class="conv-block__zh">{{ record.nameZh }}</span>
                  <span class="conv-block__id">{{ record.name }}</span>
                </div>
              </template>

              <template #repl="{ record }">
                <!-- conv-block--todo / --auto 只给输入框上色（见样式里那两条
                     .arco-input-wrapper 规则）：红=还没定替代品，绿=自动识别
                     出来的推荐；用户自己选过或填过的行不上色 -->
                <div class="conv-block" :class="`conv-block--${convReplState(record)}`">
                  <a-input
                    v-model="record.chosen"
                    :placeholder="record.suggested || '未找到推荐，请在右侧点选'"
                    size="small"
                    allow-clear
                    @input="scheduleConvZhLookup(record)"
                  />
                  <!-- 实际会用到的替代品：中文名 + 方块 ID 都写成正文。
                       只靠输入框的 placeholder 显示自动推荐时，一旦聚焦或点开
                       就看不见了，用户无法确认转换时究竟会换成哪个方块。 -->
                  <span v-if="convReplacementOf(record)" class="conv-block__eff">
                    <span class="conv-block__zh">{{ replZh(record) }}</span>
                    <span class="conv-block__id">{{ convReplacementOf(record) }}</span>
                  </span>
                </div>
              </template>

              <template #state="{ record }">
                <a-tag v-if="convIsAuto(record)" size="small" color="green">自动推荐</a-tag>
                <a-tag v-else-if="record.chosen" size="small" color="green">已指定</a-tag>
                <a-tag v-else size="small" color="red">待指定</a-tag>
              </template>
            </a-table>
          </div>

          <!-- 右栏：方块列表，点选即指定给左栏当前行 -->
          <div class="conv-split__pane conv-split__pane--pick">
            <a-input-search
              v-model="convPickQuery"
              placeholder="搜索方块：中文名 / 英文名 / ID"
              size="small"
              :loading="convPickBusy"
              allow-clear
              @input="onConvPickSearch"
              @search="onConvPickSearch"
              @clear="onConvPickSearch('')"
            />
            <div class="hint-dim" style="margin: 6px 0">
              <template v-if="convActiveItem">
                点下面的方块，即指定为
                <b>{{ convActiveItem.nameZh || convActiveItem.name }}</b> 的替代品
              </template>
              <template v-else>请先在左侧点选要替换的方块</template>
            </div>
            <div class="conv-pick">
              <a-empty
                v-if="convPickHits.length === 0"
                :description="convPickQuery.trim() ? '没有匹配的方块' : '输入关键字搜索方块'"
              />
              <button
                v-for="h in convPickHits"
                :key="h.name"
                type="button"
                class="conv-pick__item"
                :class="{
                  'is-current':
                    convActiveItem && convReplacementOf(convActiveItem) === h.name,
                  'is-other-version': !convHitInTarget(h),
                }"
                :title="
                  convHitInTarget(h)
                    ? '点一下即指定为当前行的替代品'
                    : '目标版本里没有这个方块，不能作为替代品'
                "
                @click="pickConvBlock(h)"
              >
                <span class="conv-block__zh">
                  {{ displayName({ zhName: h.zh, enName: h.en }) || h.name }}
                  <span v-if="!convHitInTarget(h)" class="conv-pick__warn">
                    目标版本没有
                  </span>
                </span>
                <!-- 英文名与方块 ID 一并给出，显示名称在上方 -->
                <span class="conv-block__id">
                  {{ h.name }}<template v-if="h.en"> · {{ h.en }}</template>
                </span>
              </button>
            </div>
          </div>
        </div>
      </template>
      <a-empty v-else description="请先选择目标版本并分析差异" />
    </a-spin>
  </a-modal>
</template>
