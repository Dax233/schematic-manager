<script setup lang="ts">
import { computed, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import type { TableColumnData, TableData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import { analyzeSchematic, buildScene, saveMetadata, scanLitematics } from "./api";
import SceneViewer from "./components/SceneViewer.vue";
import type { AnalysisResult, SceneData, SchematicSummary } from "./types";
import { formatDims, formatSize, formatStacks } from "./utils/format";

const appWindow = getCurrentWindow();

const files = ref<SchematicSummary[]>([]);
const selected = ref<SchematicSummary | null>(null);
const analysis = ref<AnalysisResult | null>(null);
const analyzing = ref(false);
const saving = ref(false);
// 元数据编辑草稿。选中文件时用文件中的值填充
const draft = ref({ name: "", author: "", description: "" });
const dirty = ref(false);

// 3D 预览
const viewer = ref<InstanceType<typeof SceneViewer> | null>(null);
const scene = ref<SceneData | null>(null);
const sceneLoading = ref(false);
const sceneError = ref("");
const brightness = ref(1.45);
const activeView = ref("left45");
const sceneInfo = computed(() => {
  if (!scene.value) return "";
  const s = scene.value;
  const parts = [`${s.totalTriangles.toLocaleString()} 个三角形`];
  if (s.unresolvedBlocks > 0) parts.push(`${s.unresolvedBlocks} 种方块无贴图`);
  return parts.join("　");
});
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
    const r = await scanLitematics(dir);
    files.value = r.items;
    const parts: string[] = [];
    const bad = r.items.filter((f) => f.error).length;
    if (bad > 0) parts.push(`${bad} 个文件解析失败`);
    // 被跳过的目录必须告知：否则结果看起来完整，用户会以为文件真的不存在
    if (r.skippedDirs > 0) {
      parts.push(`已跳过 ${r.skippedDirs} 个无法读取的目录`);
      // 列出被跳过的路径，否则用户无从判断少了哪些内容
      Message.warning({
        content: `以下目录无法读取，已跳过：\n${r.skippedPaths.join("\n")}`,
        duration: 8000,
      });
    }
    status.value = parts.length > 0 ? `扫描完成，${parts.join("，")}` : "扫描完成";
  } catch (e) {
    status.value = "扫描失败";
    Message.error(`扫描失败：${e}`);
  }
}

function resetDraft(record: SchematicSummary | null) {
  draft.value = {
    name: record?.name ?? "",
    author: record?.author ?? "",
    description: record?.description ?? "",
  };
  dirty.value = false;
}

async function saveDraft() {
  const cur = selected.value;
  if (!cur) return;
  saving.value = true;
  try {
    const out = await saveMetadata(cur.path, {
      name: draft.value.name,
      author: draft.value.author,
      description: draft.value.description,
    });
    dirty.value = false;
    // 重新读取，确保界面显示的是文件中的真实内容而不是内存里的草稿
    const fresh = await scanLitematics(cur.dirName, 1);
    const hit = fresh.items.find((f) => f.path === cur.path);
    if (hit) {
      selected.value = hit;
      resetDraft(hit);
      const idx = files.value.findIndex((f) => f.path === hit.path);
      if (idx >= 0) files.value[idx] = hit;
      Message.success(`已保存，备份在：${out.backupPath}`);
    } else {
      // 文件被移动或重命名时，界面若继续显示旧记录会误导用户
      Message.warning(
        `已保存，但重新读取时未在该目录找到该文件（可能已被移动或重命名）。备份在：${out.backupPath}`,
      );
    }
  } catch (e) {
    Message.error(`保存失败：${e}`);
  } finally {
    saving.value = false;
  }
}

// Arco 的 row-click 回调给出的是通用的 TableData，
// 这里收窄回业务类型；行数据本就来自 files，字段是一致的。
async function loadScene(record: SchematicSummary) {
  scene.value = null;
  sceneError.value = "";
  sceneLoading.value = true;
  try {
    scene.value = await buildScene(record.path);
  } catch (e) {
    sceneError.value = String(e);
  } finally {
    sceneLoading.value = false;
  }
}

function setView(name: string) {
  activeView.value = name;
  viewer.value?.applyPreset(name);
}

async function onRowClick(raw: TableData) {
  const record = raw as unknown as SchematicSummary;
  selected.value = record;
  analysis.value = null;
  resetDraft(record);
  if (record.error) return;
  void loadScene(record);
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
        <div class="preview">
          <div class="preview__stage">
            <a-spin :loading="sceneLoading" style="width: 100%; height: 100%">
              <SceneViewer
                ref="viewer"
                :scene="scene"
                :brightness="brightness"
              />
            </a-spin>
          </div>
          <div class="preview__bar">
            <a-radio-group
              v-model="activeView"
              type="button"
              size="small"
              :disabled="!scene"
              @change="setView(String(activeView))"
            >
              <a-radio value="front">正面</a-radio>
              <a-radio value="left45">左侧 45°</a-radio>
              <a-radio value="right45">右侧 45°</a-radio>
              <a-radio value="back">背面</a-radio>
            </a-radio-group>
            <span class="preview__label">亮度</span>
            <a-slider
              v-model="brightness"
              :min="0.5"
              :max="2"
              :step="0.05"
              :style="{ width: '140px' }"
            />
            <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
            <span class="preview__spacer" />
            <span v-if="sceneError" class="preview__error">{{ sceneError }}</span>
            <span v-else-if="sceneInfo" class="preview__info">{{ sceneInfo }}</span>
          </div>
        </div>
        <a-tabs default-active-key="info" style="padding: 0 8px">
          <a-tab-pane key="info" title="信息">
            <template v-if="selected">
              <a-form :model="draft" layout="vertical" size="small">
                <a-form-item label="投影名称">
                  <a-input
                    v-model="draft.name"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-form-item label="作者">
                  <a-input
                    v-model="draft.author"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-form-item label="描述">
                  <a-textarea
                    v-model="draft.description"
                    :auto-size="{ minRows: 3, maxRows: 8 }"
                    :disabled="!!selected.error"
                    @input="dirty = true"
                  />
                </a-form-item>
                <a-space>
                  <a-button
                    type="primary"
                    size="small"
                    :loading="saving"
                    :disabled="!dirty || !!selected.error"
                    @click="saveDraft"
                  >
                    保存
                  </a-button>
                  <a-button size="small" :disabled="!dirty" @click="resetDraft(selected)">
                    撤销
                  </a-button>
                  <span v-if="dirty" class="hint-warn">有未保存的修改</span>
                  <span v-else-if="!selected.error" class="hint-dim">
                    保存前会自动备份原件
                  </span>
                </a-space>
              </a-form>
              <a-divider style="margin: 12px 0" />
            </template>
            <a-descriptions
              v-if="selected"
              :column="1"
              size="small"
              :label-style="{ width: '110px' }"
              bordered
            >
              <a-descriptions-item label="文件名">{{ selected.fileName }}</a-descriptions-item>
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
              <a-descriptions-item label="待处理刻">{{ selected.pendingTicks }}</a-descriptions-item>
              <a-descriptions-item label="内含预览图">
                {{ selected.hasPreview ? "是" : "否" }}
              </a-descriptions-item>
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
                  <span>空气方块：<b>{{ analysis.airTotal }}</b></span>
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
