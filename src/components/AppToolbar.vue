/**
 * 顶部工具条：目录管理（添加/已加载下拉/移除）、搜索、
 * 以及各功能弹窗的入口按钮。
 *
 * 只负责把操作转发给 store 各域；按钮的禁用/加载态也从 store 读。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

const {
  pickDirectory,
  loadedDirs,
  dirItems,
  isDirEnabled,
  toggleDir,
  removeDir,
  searchText,
  selected,
  files,
  filteredFiles,
  dupBusy,
  scanDuplicatesInline,
  openReplace,
  openConvert,
  openBackups,
  packOnOpen,
  openPacksPanel,
  setupOnOpen,
  openSetup,
} = useAppStore();
</script>

<template>
  <div class="toolbar">
    <a-button size="small" type="primary" @click="pickDirectory">添加目录</a-button>
    <!-- 已加载目录的下拉：查看当前加载了哪些、并逐个移除。
         不禁用：未加载时点开应看到「尚未加载任何目录」的说明，
         而不是一个点不动的按钮——那样用户不知道是坏了还是没数据。 -->
    <a-dropdown trigger="click">
      <a-button size="small">
        已加载目录（{{ loadedDirs.length }}）
        <icon-down />
      </a-button>
      <template #content>
        <a-doption v-if="loadedDirs.length === 0" disabled>尚未加载任何目录</a-doption>
        <a-doption v-for="d in loadedDirs" :key="d" :value="d">
          <div class="dir-item">
            <!-- 勾选才在下方列表显示 -->
            <a-checkbox
              :model-value="isDirEnabled(d)"
              @change="toggleDir(d, $event as boolean)"
              @click.stop
            />
            <span class="dir-item__name" :title="d">{{ d }}</span>
            <span class="dir-item__count">{{ (dirItems[d] ?? []).length }} 个</span>
            <!-- 移除按钮：用 @click.stop 阻止冒泡，否则点它会同时选中该项、
                 让下拉以为是「选择目录」而不是「移除」 -->
            <a-button
              size="mini"
              type="text"
              status="danger"
              title="从列表移除该目录（不会删除磁盘上的文件）"
              @click.stop="removeDir(d)"
            >
              移除
            </a-button>
          </div>
        </a-doption>
      </template>
    </a-dropdown>
    <a-input-search
      v-model="searchText"
      size="small"
      placeholder="搜索名称/作者/文件名…"
      style="width: 240px"
      allow-clear
    />
    <a-button size="small" :disabled="!selected" @click="openReplace">方块替换</a-button>
    <a-button size="small" :disabled="!selected" @click="openConvert">版本转换</a-button>
    <a-button size="small" :loading="dupBusy" @click="scanDuplicatesInline">
      重复文件筛查
    </a-button>
    <a-button size="small" @click="openBackups">备份管理</a-button>
    <a-button size="small" :loading="packOnOpen" @click="openPacksPanel">
      资源包管理
    </a-button>
    <a-button size="small" :loading="setupOnOpen" @click="openSetup">版本库</a-button>
    <span class="preview__spacer" />
    <span class="hint-dim">
      共 {{ files.length }} 个投影<span v-if="searchText">，匹配 {{ filteredFiles.length }} 个</span>
    </span>
  </div>
</template>
