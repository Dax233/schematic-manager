/**
 * 左侧目录栏：文件操作条、重复筛查结果条、投影文件表格、
 * 底部多选操作条，以及行右键菜单。
 *
 * 右键菜单虽然浮在全窗口坐标上，但它只服务于这张表的行，
 * 因此跟着表格放在这里。宽度本身由 App 的分隔条控制（useSidebarResize）。
 * 状态在 store/fileList.ts。
 */
<script setup lang="ts">
import { useAppStore } from "../store";
import { formatDims } from "../utils/format";

const {
  sidebarWidth,
  filteredFiles,
  tableColumns,
  tableKey,
  checkedKeys,
  checkedCount,
  hasChecked,
  rowClass,
  onSelectionChange,
  onColumnResize,
  onRowClick,
  onRowContextMenu,
  fileBusy,
  doDeleteFiles,
  doCopyFiles,
  doCopyFilesTo,
  doOpenFolder,
  resetColumnWidths,
  dupResult,
  dupFileCount,
  clearDuplicateScan,
  checkAll,
  invertChecks,
  clearChecks,
  ctxMenu,
  revealRow,
} = useAppStore();
</script>

<template>
  <div class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <!-- 文件操作条：在目录栏顶部、靠左。
         作用于勾选的文件；未勾选时禁用，避免误操作到「看起来选中」的行。 -->
    <div class="sidebar__ops">
      <a-button
        size="mini"
        status="danger"
        :disabled="!hasChecked"
        :loading="fileBusy"
        @click="doDeleteFiles"
      >
        删除
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFiles">
        复制
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFilesTo">
        复制到
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="doOpenFolder">
        打开文件夹
      </a-button>
      <a-button size="mini" @click="resetColumnWidths" title="把各列宽度恢复为默认">
        列宽复位
      </a-button>
    </div>

    <!-- 重复筛查结果条：就地显示在列表上方，不再单开弹窗。
         重复行本身已按组上色（见 rowClass），这里只给汇总与清除入口。 -->
    <div v-if="dupResult" class="sidebar__dups">
      <template v-if="dupResult.groups.length > 0">
        <span class="sidebar__dups-text">
          发现 <b>{{ dupResult.groups.length }}</b> 组重复、
          <b>{{ dupFileCount }}</b> 个文件；已默认勾选多余的副本
        </span>
        <a-button size="mini" @click="clearDuplicateScan">清除标记</a-button>
      </template>
      <template v-else>
        <span class="sidebar__dups-text">没有发现内容相同的文件</span>
        <a-button size="mini" @click="clearDuplicateScan">知道了</a-button>
      </template>
    </div>
    <a-alert
      v-if="dupResult && dupResult.unreadable.length > 0"
      type="warning"
      style="margin: 0 0 4px; flex: 0 0 auto"
    >
      有 {{ dupResult.unreadable.length }} 个文件无法读取，未参与判定
    </a-alert>

    <a-table
      :key="tableKey"
      :columns="tableColumns"
      :data="filteredFiles"
      :pagination="false"
      row-key="path"
      size="small"
      :bordered="{ wrapper: true, cell: false }"
      :scroll="{ y: '100%' }"
      column-resizable
      :row-selection="{ type: 'checkbox', showCheckedAll: false, width: 40 }"
      :selected-keys="checkedKeys"
      :row-class="rowClass"
      @selection-change="onSelectionChange"
      @column-resize="onColumnResize"
      @row-click="onRowClick"
      @row-contextmenu="onRowContextMenu"
    >
      <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
      <template #empty>
        <a-empty description="点击「添加目录」选择投影所在文件夹" />
      </template>
    </a-table>

    <!-- 目录栏底部：只放多选操作 -->
    <div class="sidebar__checks">
      <span class="hint-dim">已选 {{ checkedCount }} / {{ filteredFiles.length }}</span>
      <span class="preview__spacer" />
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="checkAll">
        全选
      </a-button>
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="invertChecks">
        反选
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="clearChecks">清除</a-button>
    </div>

    <!-- 行的右键菜单：只保留「打开文件夹」 -->
    <div
      v-if="ctxMenu.open"
      class="ctx-menu"
      :style="{ left: ctxMenu.x + 'px', top: ctxMenu.y + 'px' }"
      role="menu"
      @contextmenu.prevent
    >
      <div
        class="ctx-menu__item"
        role="menuitem"
        tabindex="0"
        @click="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
        @keydown.enter="revealRow({ path: ctxMenu.path, name: ctxMenu.name })"
      >
        打开文件夹
      </div>
    </div>
  </div>
</template>
