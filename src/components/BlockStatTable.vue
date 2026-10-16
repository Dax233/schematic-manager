/**
 * 方块统计表（详情面板「方块」页签的内容）。
 *
 * 列宽拖动、分页图标懒加载、点行填入替换规则这三件事都只服务于这张表，
 * 独立成组件后详情面板不用背这些细节。状态在 store/scenePreview.ts，
 * 点行的回填动作在 store/blockReplace.ts（onBlockRowClick）。
 */
<script setup lang="ts">
import { useAppStore } from "../store";
import { formatStacks } from "../utils/format";

const {
  analysis,
  analyzing,
  blockTableKey,
  blockTableColumns,
  BLOCK_PAGE_SIZE,
  blockPage,
  blockRows,
  blockStateText,
  iconOf,
  iconErrorOf,
  copyBlockTable,
  exportBlockTable,
  resetBlockColumnWidths,
  onBlockColumnResize,
  onBlockRowClick,
} = useAppStore();
</script>

<template>
  <a-spin :loading="analyzing" style="width: 100%; height: 100%">
    <template v-if="analysis">
      <div class="block-head">
        <a-space :size="16">
          <span>非空气方块：<b>{{ analysis.nonAirTotal }}</b></span>
          <span>空气方块：<b>{{ analysis.airTotal }}</b></span>
          <span>方块种类：<b>{{ analysis.distinctBlockTypes }}</b></span>
          <span>状态种类：<b>{{ analysis.distinctStates }}</b></span>
        </a-space>
        <span class="preview__spacer" />
        <a-button size="mini" @click="copyBlockTable">复制表格</a-button>
        <a-button size="mini" @click="exportBlockTable">导出表格</a-button>
        <a-button
          size="mini"
          title="把各列宽度恢复为默认"
          @click="resetBlockColumnWidths"
        >
          列宽复位
        </a-button>
      </div>
      <div class="hint-dim" style="margin-bottom: 6px">
        数量按建造所需的物品折算：水与岩浆只算源头一个桶、门与床只算一次、双层台阶算两个，
        摆法差异（墙上的火把等）与同一物品合为一行；点击任意行可以把它设为方块替换的源方块，表头可拖动调整列宽
      </div>
      <a-table
        :key="blockTableKey"
        class="block-table"
        :columns="blockTableColumns"
        :data="blockRows"
        :pagination="{ pageSize: BLOCK_PAGE_SIZE, current: blockPage }"
        row-key="state"
        size="small"
        column-resizable
        @column-resize="onBlockColumnResize"
        @page-change="(p: number) => (blockPage = p)"
        @row-click="onBlockRowClick"
        :scroll="{ y: '100%' }"
      >
        <template #icon="{ record }">
          <img
            v-if="iconOf(record.state)"
            class="block-icon"
            :src="iconOf(record.state)"
            :alt="record.zhName || record.name"
            :title="record.zhName || record.name"
          />
          <!-- 画不出来时说清楚原因，而不是留一个让人以为是加载慢的空位 -->
          <span
            v-else-if="iconErrorOf(record.state)"
            class="block-icon block-icon--failed"
            :title="iconErrorOf(record.state)"
            >无图</span
          >
        </template>
        <template #zhName="{ record }">
          <span v-if="record.zhName">{{ record.zhName }}</span>
          <span v-else class="hint-dim" :title="record.enName || '语言表中没有这一项'">
            {{ record.enName || "—" }}
          </span>
        </template>
        <template #stacks="{ record }">{{ formatStacks(record.count) }}</template>
        <!-- 合并显示的行写明合并了多少种状态：那一行的数量是所有状态的总和，
             只写一个代表状态会让人以为「只有这么多是那个状态的」 -->
        <template #state="{ record }">{{ blockStateText(record) }}</template>
      </a-table>
    </template>
    <a-empty v-else description="选择投影文件后显示方块统计" />
  </a-spin>
</template>
