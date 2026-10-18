/**
 * 顶部工具条：目录管理（添加/已加载下拉/移除）、搜索、
 * 以及各功能弹窗的入口按钮；最右侧是界面语言切换。
 *
 * 只负责把操作转发给 store 各域；按钮的禁用/加载态也从 store 读。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { LOCALES, currentLocale, setLocale } from "../i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

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
  setupOnOpen,
  openSetup,
  openUpdate,
} = useAppStore();

/** 当前语言的按钮文字：认不出来的语言取清单里第一项，与 i18n 的回落一致 */
function currentLabelKey(): string {
  return LOCALES.find((item) => item.locale === currentLocale())!.labelKey;
}
</script>

<template>
  <div class="toolbar">
    <a-button size="small" type="primary" @click="pickDirectory">
      {{ t("toolbar.addDir") }}
    </a-button>
    <!-- 已加载目录的下拉：查看当前加载了哪些、并逐个移除。
         不禁用：未加载时点开应看到「尚未加载任何目录」的说明，
         而不是一个点不动的按钮——那样用户不知道是坏了还是没数据。 -->
    <a-dropdown trigger="click">
      <a-button size="small">
        {{ t("toolbar.loadedDirs", { count: loadedDirs.length }) }}
        <icon-down />
      </a-button>
      <template #content>
        <a-doption v-if="loadedDirs.length === 0" disabled>
          {{ t("toolbar.noDirs") }}
        </a-doption>
        <a-doption v-for="d in loadedDirs" :key="d" :value="d">
          <div class="dir-item">
            <!-- 勾选才在下方列表显示 -->
            <a-checkbox
              :model-value="isDirEnabled(d)"
              @change="toggleDir(d, $event as boolean)"
              @click.stop
            />
            <span class="dir-item__name" :title="d">{{ d }}</span>
            <span class="dir-item__count">
              {{ t("toolbar.fileCount", { count: (dirItems[d] ?? []).length }) }}
            </span>
            <!-- 移除按钮：用 @click.stop 阻止冒泡，否则点它会同时选中该项、
                 让下拉以为是「选择目录」而不是「移除」 -->
            <a-button
              size="mini"
              type="text"
              status="danger"
              :title="t('toolbar.removeTitle')"
              @click.stop="removeDir(d)"
            >
              {{ t("toolbar.remove") }}
            </a-button>
          </div>
        </a-doption>
      </template>
    </a-dropdown>
    <a-input-search
      v-model="searchText"
      size="small"
      :placeholder="t('toolbar.searchPlaceholder')"
      style="width: 240px"
      allow-clear
    />
    <a-button size="small" :disabled="!selected" @click="openReplace">
      {{ t("toolbar.replace") }}
    </a-button>
    <a-button size="small" :disabled="!selected" @click="openConvert">
      {{ t("toolbar.convert") }}
    </a-button>
    <a-button size="small" :loading="dupBusy" @click="scanDuplicatesInline">
      {{ t("toolbar.duplicates") }}
    </a-button>
    <a-button size="small" @click="openBackups">{{ t("toolbar.backups") }}</a-button>
    <a-button size="small" :loading="setupOnOpen" @click="openSetup">
      {{ t("toolbar.setup") }}
    </a-button>
    <a-button size="small" @click="openUpdate">{{ t("toolbar.update") }}</a-button>
    <!-- RMC投影站：按用户要求先只放按钮，点击不做任何事，
         这个入口预留给附加程序接入。
         TODO(附加程序接入后)：接上附加程序提供的入口。完成条件是该按钮能
         打开 RMC 投影站（或唤起附加程序）且失败时有明确提示；在此之前
         保持无行为，不显示任何「点不动的按钮」式的伪交互。
         跟踪方式：随附加程序的开发进度一并处理，仓库内暂无对应任务号。 -->
    <a-button size="small" :title="t('toolbar.rmcTitle')">{{ t("toolbar.rmc") }}</a-button>
    <span class="preview__spacer" />
    <span class="hint-dim">
      {{ t("toolbar.totalFiles", { total: files.length }) }}<span v-if="searchText">{{
        t("toolbar.matchedFiles", { matched: filteredFiles.length })
      }}</span>
    </span>
    <!-- 语言切换：放最右侧，与标题栏、状态栏同一条视觉轴 -->
    <a-dropdown trigger="click" position="br">
      <a-button size="small" :title="t('lang.label')">
        <icon-language />
        {{ t(currentLabelKey()) }}
      </a-button>
      <template #content>
        <a-doption
          v-for="item in LOCALES"
          :key="item.locale"
          :value="item.locale"
          @click="setLocale(item.locale)"
        >
          {{ t(item.labelKey) }}
        </a-doption>
      </template>
    </a-dropdown>
  </div>
</template>
