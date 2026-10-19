/**
 * 备份与日志抽屉：备份清单（还原/删除）与全局运行日志两个页签。
 *
 * 状态与动作都在 store/backups.ts，这里只做展示与触发。
 *
 * 抽屉上下各留 16px（见 global.scss 的 .arco-drawer）：贴满整屏高度会像页面被
 * 切掉一块，与其它弹出层的处理也不一致。
 * 需要滚动的是内容区：运行日志可以很长，放在 Arco 的无轨道滚动条里。
 */
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import RunLogList from "./RunLogList.vue";
import { formatSize } from "../utils/format";

const { t } = useI18n();

const {
  selected,
  backupOpen,
  backups,
  backupLoading,
  logs,
  logLoading,
  formatTime,
  refreshBackups,
  refreshLogs,
  openLogs,
  onRestore,
  onDelete,
} = useAppStore();

/** 列定义随语言变化，写在这里而不是常量里 */
const columns = computed(() => [
  { title: t("backup.colTime"), slotName: "time", width: 170 },
  { title: t("backup.colOp"), dataIndex: "op", width: 110 },
  { title: t("backup.colSize"), slotName: "size", width: 90 },
  { title: t("backup.colAct"), slotName: "act", width: 150 },
]);
</script>

<template>
  <a-drawer
    v-model:visible="backupOpen"
    :title="t('backup.title')"
    :width="620"
    :footer="false"
    unmount-on-close
  >
    <a-tabs default-active-key="backups">
      <a-tab-pane key="backups" :title="t('backup.tabBackups', { count: backups.length })">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshBackups">{{ t("backup.refresh") }}</a-button>
          <span v-if="selected" class="hint-dim">
            {{ t("backup.onlyFor", { name: selected.fileName }) }}
          </span>
        </a-space>
        <a-table
          :columns="columns"
          :data="backups"
          :loading="backupLoading"
          :pagination="{ pageSize: 12 }"
          row-key="path"
          size="small"
        >
          <template #time="{ record }">{{ formatTime(record.createdNs) }}</template>
          <template #size="{ record }">{{ formatSize(record.size) }}</template>
          <template #act="{ record }">
            <a-space>
              <a-link :disabled="!selected" @click="onRestore(record)">
                {{ t("backup.restore") }}
              </a-link>
              <a-popconfirm :content="t('backup.deleteConfirm')" @ok="onDelete(record)">
                <a-link status="danger">{{ t("backup.delete") }}</a-link>
              </a-popconfirm>
            </a-space>
          </template>
          <template #empty>
            <a-empty :description="selected ? t('backup.noBackups') : t('backup.pickFile')" />
          </template>
        </a-table>
      </a-tab-pane>

      <a-tab-pane key="logs" :title="t('backup.tabLogs')">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshLogs">{{ t("backup.refresh") }}</a-button>
          <!-- 日志管理窗口（清空、打开日志目录）从这里进：它原先挂在资源包
               管理窗口上，那个窗口已移除；日志属于备份与运维这条线，放在
               同一条运行日志页签里最顺手 -->
          <a-button size="small" @click="openLogs">{{ t("backup.logManage") }}</a-button>
        </a-space>
        <!-- 日志可能很长：内容区自己滚（Arco 无轨道滚动条）。
             抽屉内容区没有固定高度，这里给滚动条一个显式上限；同样要靠
             .scroll-fill 把高度传给内部容器，否则它会被内容撑开而不滚。 -->
        <a-scrollbar
          type="embed"
          class="scroll-fill__inner"
          outer-class="scroll-fill"
          :outer-style="{ maxHeight: 'calc(100vh - 220px)' }"
        >
          <a-spin :loading="logLoading" style="width: 100%">
            <a-empty v-if="logs.length === 0" :description="t('log.empty')" />
            <RunLogList v-else :lines="logs" />
          </a-spin>
        </a-scrollbar>
      </a-tab-pane>
    </a-tabs>
  </a-drawer>
</template>
