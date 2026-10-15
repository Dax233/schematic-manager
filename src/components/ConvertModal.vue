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
import type { TableData } from "@arco-design/web-vue";
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
  runConvertAnalyze,
  doConvert,
} = useAppStore();

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
                  {{ h.zh || h.en || h.name }}
                  <span v-if="!convHitInTarget(h)" class="conv-pick__warn">
                    目标版本没有
                  </span>
                </span>
                <!-- 英文名与方块 ID 一并给出，中文名在上方 -->
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
