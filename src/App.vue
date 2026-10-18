<script setup lang="ts">
/**
 * 应用根组件：只做装配。
 *
 * 业务状态与逻辑全部在 store/ 的各域里，界面分区各自成组件
 * （TitleBar / AppToolbar / DirSidebar / DetailPanel 与各弹窗）。
 * 这里只保留三处跨分区的连接：
 * 1. 创建并 provide 应用状态（store/index.ts 的 provideAppStore）；
 * 2. 跟随系统主题（composables/useTheme）；
 * 3. 目录栏与详情面板之间的拖动条——它同时依赖两侧布局，不属于任何一侧。
 */
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import AppToolbar from "./components/AppToolbar.vue";
import BackupDrawer from "./components/BackupDrawer.vue";
import ConvertModal from "./components/ConvertModal.vue";
import DetailPanel from "./components/DetailPanel.vue";
import DirSidebar from "./components/DirSidebar.vue";
import LogModal from "./components/LogModal.vue";
import ReplaceModal from "./components/ReplaceModal.vue";
import SetupModal from "./components/SetupModal.vue";
import TitleBar from "./components/TitleBar.vue";
import UpdateModal from "./components/UpdateModal.vue";
import { useTheme } from "./composables/useTheme";
import { SIDEBAR_MAX_W, SIDEBAR_MIN_W } from "./composables/useSidebarResize";
import { arcoLocaleOf, currentLocale } from "./i18n";
import { provideAppStore } from "./store";

// 主题跟随在挂载前启动：它异步读取并订阅系统主题，与状态装配无关
const { followSystemTheme } = useTheme();

// 语言切换要连 Arco 自己的内置文案（分页、空数据、弹窗按钮）一起换
const arcoLocale = computed(() => arcoLocaleOf(currentLocale()));

const { t } = useI18n();

const s = provideAppStore();

// 只剩拖动条与状态栏要从 store 取数，其余绑定都在各分区组件内部解构
const {
  sidebarWidth, resizing, startResize, toggleSidebarWidth, onSplitterKey,
  selected, statusText,
} = s;

// 启动即跟随系统主题；不等它完成，免得拖慢首屏
void followSystemTheme();
</script>

<template>
  <a-config-provider :locale="arcoLocale">
    <div class="app-shell">
      <TitleBar />

      <AppToolbar />

      <div class="workbench" :class="{ 'is-resizing': resizing }">
        <DirSidebar />

        <!-- 拖动条：调整左侧目录栏宽度 -->
        <div
          class="splitter"
          role="separator"
          aria-orientation="vertical"
          :aria-label="t('app.splitterAria')"
          :aria-valuenow="sidebarWidth"
          :aria-valuemin="SIDEBAR_MIN_W"
          :aria-valuemax="SIDEBAR_MAX_W"
          tabindex="0"
          :title="t('app.splitterTitle')"
          @pointerdown="startResize"
          @dblclick="toggleSidebarWidth"
          @keydown="onSplitterKey"
        />

        <DetailPanel />
      </div>

      <ConvertModal />

      <SetupModal />

      <LogModal />

      <ReplaceModal />

      <UpdateModal />

      <BackupDrawer />

      <div class="statusbar">
        <span>{{ statusText }}</span>
        <span class="statusbar__spacer" />
        <span v-if="selected">{{ t("app.selectedFile", { name: selected.fileName }) }}</span>
      </div>
    </div>
  </a-config-provider>
</template>
