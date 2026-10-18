/**
 * 左侧目录栏：文件操作条、重复筛查结果条、投影文件表格、
 * 底部多选操作条，以及行右键菜单。
 *
 * 右键菜单虽然浮在全窗口坐标上，但它只服务于这张表的行，
 * 因此跟着表格放在这里。宽度本身由 App 的分隔条控制（useSidebarResize）。
 * 状态在 store/fileList.ts。
 *
 * 表格的纵向滚动由 Arco Table 自带的滚动条负责（它内部就用 Arco 的
 * Scrollbar），轨道已在全局样式里去掉，不留原生滚动条。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import { formatDims } from "../utils/format";

const { t } = useI18n();

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
        {{ t("files.delete") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFiles">
        {{ t("files.copy") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" :loading="fileBusy" @click="doCopyFilesTo">
        {{ t("files.copyTo") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="doOpenFolder">
        {{ t("files.openFolder") }}
      </a-button>
      <a-button size="mini" @click="resetColumnWidths" :title="t('files.resetWidthsTitle')">
        {{ t("files.resetWidths") }}
      </a-button>
    </div>

    <!-- 重复筛查结果条：就地显示在列表上方，不再单开弹窗。
         重复行已排到列表最前并按组上色（见 rowClass 与 #fileName 的组号），
         这里只给汇总与清除入口。 -->
    <div v-if="dupResult" class="sidebar__dups">
      <span class="sidebar__dups-text">
        <template v-if="dupResult.groups.length > 0">
          <i18n-t scope="global" keypath="files.dupFound" tag="span">
            <template #groups><b>{{ dupResult.groups.length }}</b></template>
            <template #files><b>{{ dupFileCount }}</b></template>
          </i18n-t>
        </template>
        <template v-else>{{ t("files.dupNotFound") }}</template>
        <template v-if="dupSuspects.length > 0">
          <i18n-t scope="global" keypath="files.dupSuspects" tag="span">
            <template #groups><b>{{ dupSuspects.length }}</b></template>
            <template #files>{{ suspectFileCount }}</template>
          </i18n-t>
        </template>
      </span>
      <a-button size="mini" @click="clearDuplicateScan">
        {{ dupResult.groups.length > 0 ? t("files.clearMarks") : t("files.gotIt") }}
      </a-button>
    </div>
    <a-alert
      v-if="dupResult && (dupResult.unreadable.length > 0 || dupResult.skippedDirs > 0)"
      type="warning"
      style="margin: 0 0 4px; flex: 0 0 auto"
    >
      <template v-if="dupResult.unreadable.length > 0">
        {{ t("files.unreadableFiles", { count: dupResult.unreadable.length }) }}
      </template>
      <template v-if="dupResult.unreadable.length > 0 && dupResult.skippedDirs > 0">
        {{ t("files.unreadableBothSeparator") }}
      </template>
      <template v-if="dupResult.skippedDirs > 0">
        {{ t("files.unreadableDirs", { count: dupResult.skippedDirs }) }}
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
            :title="t('files.dupGroupTitle', { no: dupNoOf(record.path) })"
          >{{ dupNoOf(record.path) }}</span>
          <span
            v-else-if="isSuspect(record.path)"
            class="dup-chip dup-chip--suspect"
            :title="t('files.suspectTitle')"
          >{{ t("files.suspectChip") }}</span>
          <!-- 名称列关掉了 Arco 的 tooltip（它量不准插槽内容），
               完整文件名只能在这里自己给出来 -->
          <span class="dup-name__text" :title="record.fileName">{{ record.fileName }}</span>
        </span>
      </template>
      <template #dims="{ record }">{{ formatDims(record.enclosing) }}</template>
      <template #empty>
        <a-empty :description="t('files.empty')" />
      </template>
    </a-table>

    <!-- 目录栏底部：只放多选操作 -->
    <div class="sidebar__checks">
      <span class="hint-dim">
        {{ t("files.checkedCount", { checked: checkedCount, total: filteredFiles.length }) }}
      </span>
      <span class="preview__spacer" />
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="checkAll">
        {{ t("files.checkAll") }}
      </a-button>
      <a-button size="mini" :disabled="filteredFiles.length === 0" @click="invertChecks">
        {{ t("files.invert") }}
      </a-button>
      <a-button size="mini" :disabled="!hasChecked" @click="clearChecks">
        {{ t("files.clear") }}
      </a-button>
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
        {{ t("files.openFolder") }}
      </div>
    </div>
  </div>
</template>
