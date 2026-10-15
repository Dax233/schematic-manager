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
  dupSuspects,
  dupFileCount,
  suspectFileCount,
  dupNoOf,
  dupColorNo,
  isDup,
  isSuspect,
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
         重复行已排到列表最前并按组上色（见 rowClass 与 #fileName 的组号），
         这里只给汇总与清除入口。 -->
    <div v-if="dupResult" class="sidebar__dups">
      <span class="sidebar__dups-text">
        <template v-if="dupResult.groups.length > 0">
          发现 <b>{{ dupResult.groups.length }}</b> 组内容相同的重复、共
          <b>{{ dupFileCount }}</b> 个文件（已排到最前，多余的副本已勾选）
        </template>
        <template v-else>没有发现内容相同的重复</template>
        <template v-if="dupSuspects.length > 0">
          ；另有 <b>{{ dupSuspects.length }}</b> 组疑似重复（名称/尺寸/方块数相同、
          共 {{ suspectFileCount }} 个文件），内容并不相同，未勾选
        </template>
      </span>
      <a-button size="mini" @click="clearDuplicateScan">
        {{ dupResult.groups.length > 0 ? "清除标记" : "知道了" }}
      </a-button>
    </div>
    <a-alert
      v-if="dupResult && (dupResult.unreadable.length > 0 || dupResult.skippedDirs > 0)"
      type="warning"
      style="margin: 0 0 4px; flex: 0 0 auto"
    >
      <template v-if="dupResult.unreadable.length > 0">
        有 {{ dupResult.unreadable.length }} 个文件无法读取，未参与判定
      </template>
      <template v-if="dupResult.unreadable.length > 0 && dupResult.skippedDirs > 0">；</template>
      <template v-if="dupResult.skippedDirs > 0">
        有 {{ dupResult.skippedDirs }} 个子目录无法读取，其中的文件未参与判定
      </template>
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
      <template #fileName="{ record }">
        <span class="dup-name">
          <!-- 组号色块与行底色同色：颜色负责「哪几份是一组」，
               数字负责在颜色循环复用时仍能分辨，两者互为佐证。 -->
          <span
            v-if="isDup(record.path)"
            class="dup-chip"
            :class="`dup-chip-${dupColorNo(record.path)}`"
            :title="'重复组 ' + dupNoOf(record.path) + '：与同组文件内容完全相同'"
          >{{ dupNoOf(record.path) }}</span>
          <span
            v-else-if="isSuspect(record.path)"
            class="dup-chip dup-chip--suspect"
            title="疑似重复：名称、尺寸、方块总数相同，但内容不同，请自行核对"
          >疑似</span>
          <!-- 名称列关掉了 Arco 的 tooltip（它量不准插槽内容），
               完整文件名只能在这里自己给出来 -->
          <span class="dup-name__text" :title="record.fileName">{{ record.fileName }}</span>
        </span>
      </template>
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
