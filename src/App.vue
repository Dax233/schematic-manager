<script setup lang="ts">
import { ref, onMounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const appWindow = getCurrentWindow();

const files = ref<Array<Record<string, unknown>>>([]);
const status = ref("就绪");

// 用 TableColumnData 显式标注，避免字面量推断与 Arco 类型定义冲突
import type { TableColumnData } from "@arco-design/web-vue";

const columns: TableColumnData[] = [
  { title: "名称", dataIndex: "name", width: 200, sortable: { sortDirections: ["ascend", "descend"] } },
  { title: "制作版本", dataIndex: "mcVersion", width: 100, sortable: { sortDirections: ["ascend", "descend"] } },
  { title: "尺寸", dataIndex: "size", width: 140 },
  { title: "来源目录", dataIndex: "dir", ellipsis: true, tooltip: true },
];

async function minimize() { await appWindow.minimize(); }
async function toggleMaximize() { await appWindow.toggleMaximize(); }
async function close() { await appWindow.close(); }

onMounted(() => {
  status.value = "等待扫描目录";
});
</script>

<template>
  <div class="app-shell">
    <!-- 标题栏：Tauri 无边框窗口，窗口按钮由前端绘制 -->
    <div class="titlebar" data-tauri-drag-region>
      <div class="titlebar__logo">图</div>
      <div class="titlebar__title">投影文件管理器</div>
      <div class="titlebar__spacer" data-tauri-drag-region />
      <div class="titlebar__btn" @click="minimize">&#x2500;</div>
      <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
      <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
    </div>

    <!-- 工具栏 -->
    <div class="toolbar">
      <a-button size="small" type="primary">添加目录</a-button>
      <a-button size="small">重新扫描</a-button>
      <a-button size="small">重复文件查找</a-button>
      <a-button size="small">贴图资源</a-button>
      <a-button size="small">帮助</a-button>
      <a-input-search
        size="small"
        placeholder="搜索名称/作者/文件名…"
        style="width: 240px; margin-left: 12px"
      />
    </div>

    <div class="workbench">
      <!-- 左侧文件列表 -->
      <div class="sidebar">
        <a-table
          :columns="columns"
          :data="files"
          :pagination="false"
          row-key="path"
          size="small"
          :scroll="{ y: '100%' }"
          :bordered="{ wrapper: true, cell: false }"
          :row-selection="{ type: 'checkbox' }"
        >
          <template #empty>
            <a-empty description="尚未添加扫描目录" />
          </template>
        </a-table>
      </div>

      <!-- 右侧详情 -->
      <div class="detail">
        <a-tabs default-active-key="info" style="padding: 0 8px">
          <a-tab-pane key="info" title="信息">
            <a-empty description="选择一个投影文件以查看详情" />
          </a-tab-pane>
          <a-tab-pane key="blocks" title="方块" />
          <a-tab-pane key="entities" title="实体" />
        </a-tabs>
      </div>
    </div>

    <div class="statusbar">
      <span>{{ status }}</span>
      <span class="statusbar__spacer" />
      <span>共 {{ files.length }} 个投影文件</span>
    </div>
  </div>
</template>
