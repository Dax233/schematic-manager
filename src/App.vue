<script setup lang="ts">
import { computed, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import type { TableColumnData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import { analyzeSchematic, scanLitematics } from "./api";
import type { AnalysisResult, SchematicSummary } from "./types";
import { formatDims, formatSize, formatStacks } from "./utils/format";

const appWindow = getCurrentWindow();

const files = ref<SchematicSummary[]>([]);
const selected = ref<SchematicSummary | null>(null);
const analysis = ref<AnalysisResult | null>(null);
const analyzing = ref(false);
const status = ref("就绪");

const columns: TableColumnData[] = [
  { title: "名称", dataIndex: "fileName", width: 240, ellipsis: true, tooltip: true },
  { title: "制作版本", dataIndex: "mcVersionName", width: 100 },
  { title: "尺寸", slotName: "dims", width: 130 },
  { title: "方块总数", dataIndex: "totalBlocks", width: 100 },
  { title: "来源目录", dataIndex: "dirName", ellipsis: true, tooltip: true },
];

const blockColumns: TableColumnData[] = [
  { title: "方块", dataIndex: "name", ellipsis: true, tooltip: true },
  { title: "状态", dataIndex: "state", ellipsis: true, tooltip: true },
  { title: "数量", dataIndex: "count", width: 90 },
  { title: "堆叠", slotName: "stacks", width: 110 },
];

const entityColumns: TableColumnData[] = [
  { title: "实体ID", dataIndex: "id" },
  { title: "数量", dataIndex: "count", width: 90 },
];

const statusText = computed(() => {
  const n = files.value.length;
  return n === 0 ? status.value : `${status.value}　共 ${n} 个投影文件`;
});

async function pickDirectory() {
  const dir = await open({ directory: true, multiple: false, title: "选择投影目录" });
  if (typeof dir !== "string") return;
  status.value = "正在扫描…";
  try {
    files.value = await scanLitematics(dir);
    const bad = files.value.filter((f) => f.error).length;
    status.value = bad > 0 ? `扫描完成，其中 ${bad} 个文件解析失败` : "扫描完成";
  } catch (e) {
    status.value = "扫描失败";
    Message.error(`扫描失败：${e}`);
  }
}

async function onRowClick(record: SchematicSummary) {
  selected.value = record;
  analysis.value = null;
  if (record.error) return;
  analyzing.value = true;
  try {
    analysis.value = await analyzeSchematic(record.path);
  } catch (e) {
    Message.error(`解析失败：${e}`);
  } finally {
    analyzing.value = false;
  }
}

const minimize = () => appWindow.minimize();
const toggleMaximize = () => appWindow.toggleMaximize();
const close = () => appWindow.close();
</script>

<template>
  <div class="app-shell">
    <div class="titlebar" data-tauri-drag-region>
      <div class="titlebar__logo">图</div>
      <div class="titlebar__title">投影文件管理器</div>
      <div class="titlebar__spacer" data-tauri-drag-region />
      <div class="titlebar__btn" @click="minimize">&#x2500;</div>
      <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
      <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
    </div>

    <div class="toolbar">
      <a-button size="small" type="primary" @click="pickDirectory">添加目录</a-button>
      <a-input-search size="small" placeholder="搜索名称/作者/文件名…" style="width: 240px" />
    </div>

    <div class="workbench">
      <div class="sidebar">
        <a-table
          :columns="columns"
          :data="files"
          :pagination="false"
          row-key="path"
          size="small"
          :bordered="{ wrapper: true, cell: false }"
          :scroll="{ y: '100%' }"
          @row-click="onRowClick"
        >
          <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
          <template #empty>
            <a-empty description="点击「添加目录」选择投影所在文件夹" />
          </template>
        </a-table>
      </div>

      <div class="detail">
        <a-tabs default-active-key="info" style="padding: 0 8px">
          <a-tab-pane key="info" title="信息">
            <a-descriptions
              v-if="selected"
              :column="1"
              size="small"
              :label-style="{ width: '110px' }"
              bordered
            >
              <a-descriptions-item label="文件名">{{ selected.fileName }}</a-descriptions-item>
              <a-descriptions-item label="投影名称">{{ selected.name || "—" }}</a-descriptions-item>
              <a-descriptions-item label="作者">{{ selected.author || "—" }}</a-descriptions-item>
              <a-descriptions-item label="描述">{{ selected.description || "—" }}</a-descriptions-item>
              <a-descriptions-item label="制作版本">
                {{ selected.mcVersionName || "未知" }}（数据版本 {{ selected.dataVersion }}）
              </a-descriptions-item>
              <a-descriptions-item label="投影格式">v{{ selected.formatVersion }}</a-descriptions-item>
              <a-descriptions-item label="尺寸(X×Y×Z)">
                {{ formatDims(selected.enclosing) }}
              </a-descriptions-item>
              <a-descriptions-item label="区域数量">{{ selected.regionCount }}</a-descriptions-item>
              <a-descriptions-item label="方块总数">{{ selected.totalBlocks }}</a-descriptions-item>
              <a-descriptions-item label="实体数量">{{ selected.entityCount }}</a-descriptions-item>
              <a-descriptions-item label="方块实体">{{ selected.tileEntityCount }}</a-descriptions-item>
              <a-descriptions-item label="文件大小">{{ formatSize(selected.fileSize) }}</a-descriptions-item>
              <a-descriptions-item v-if="selected.error" label="解析错误">
                <span style="color: rgb(var(--danger-6))">{{ selected.error }}</span>
              </a-descriptions-item>
            </a-descriptions>
            <a-empty v-else description="选择一个投影文件以查看详情" />
          </a-tab-pane>

          <a-tab-pane key="blocks" title="方块">
            <a-spin :loading="analyzing" style="width: 100%">
              <template v-if="analysis">
                <a-space style="margin-bottom: 8px" :size="16">
                  <span>非空气方块：<b>{{ analysis.nonAirTotal }}</b></span>
                  <span>方块种类：<b>{{ analysis.distinctBlockTypes }}</b></span>
                  <span>状态种类：<b>{{ analysis.distinctStates }}</b></span>
                </a-space>
                <a-table
                  :columns="blockColumns"
                  :data="analysis.blocks"
                  :pagination="{ pageSize: 20 }"
                  row-key="state"
                  size="small"
                  :scroll="{ y: 360 }"
                >
                  <template #stacks="{ record }">{{ formatStacks(record.count) }}</template>
                </a-table>
              </template>
              <a-empty v-else description="选择投影文件后显示方块统计" />
            </a-spin>
          </a-tab-pane>

          <a-tab-pane key="entities" title="实体">
            <template v-if="analysis">
              <a-table
                :columns="entityColumns"
                :data="analysis.entities"
                :pagination="false"
                row-key="id"
                size="small"
              />
              <a-empty
                v-if="analysis.entities.length === 0"
                description="该投影不含实体"
              />
            </template>
            <a-empty v-else description="选择投影文件后显示实体统计" />
          </a-tab-pane>
        </a-tabs>
      </div>
    </div>

    <div class="statusbar">
      <span>{{ statusText }}</span>
      <span class="statusbar__spacer" />
      <span v-if="selected">已选：{{ selected.fileName }}</span>
    </div>
  </div>
</template>
