/**
 * 版本转换弹窗。
 *
 * 向导式流程：选目标版本 → 分析差异 → 指定替代品 → 执行转换。
 * 状态与流程都在 store/versionConvert.ts，这里只做展示与触发。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

const {
  selected,
  convOpen,
  convBusy,
  convTargets,
  convTargetIdx,
  convItems,
  convAnalyzed,
  convColumns,
  runConvertAnalyze,
  doConvert,
} = useAppStore();
</script>

<template>
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
</template>
