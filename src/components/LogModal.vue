/**
 * 日志管理窗口：查看全局操作日志，并提供清空与定位日志文件两个管理动作。
 *
 * 与「备份与日志」里的操作日志页签是同一份数据（store/backups.ts 的 logs），
 * 这里只是把它做成可以单独打开的窗口——入口就在那个页签里，
 * 便于在窗口里完成清空与定位日志文件。
 *
 * 清空是不可撤销的，因此走 a-popconfirm 二次确认；日志文件本身保留
 * （见后端 backup::clear_log），所以「打开日志目录」在清空后仍然可用。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";
import OperationLogList from "./OperationLogList.vue";

const { t } = useI18n();

const {
  logOpen,
  logs,
  logLoading,
  logMaxLines,
  refreshLogs,
  clearLogs,
  openLogDir,
} = useAppStore();
</script>

<template>
  <a-modal
    v-model:visible="logOpen"
    :title="t('log.title')"
    :width="760"
    :footer="false"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    unmount-on-close
  >
    <!-- 只列后端真的会写日志的操作（commands.rs 里 append_log 的调用点）：
         说多了就是错误陈述，用户会照着这句话去找一条并不存在的记录。 -->
    <div class="hint-dim" style="margin-bottom: 8px">
      {{ t("log.hint", { count: logMaxLines }) }}
    </div>

    <a-space style="margin-bottom: 8px">
      <a-button size="small" :loading="logLoading" @click="refreshLogs">
        {{ t("log.refresh") }}
      </a-button>
      <a-button size="small" :title="t('log.openDirTitle')" @click="openLogDir">
        {{ t("log.openDir") }}
      </a-button>
      <a-popconfirm
        :content="t('log.clearConfirm')"
        :ok-text="t('log.clearOk')"
        @ok="clearLogs"
      >
        <a-button size="small" status="danger" :disabled="logs.length === 0">
          {{ t("log.clear") }}
        </a-button>
      </a-popconfirm>
      <span class="hint-dim">{{ t("log.count", { count: logs.length }) }}</span>
    </a-space>

    <!-- 日志可以很长：内容区自己滚（Arco 无轨道滚动条），标题与按钮留在视野里 -->
    <a-scrollbar type="embed" :outer-style="{ flex: '1 1 auto', minHeight: 0 }">
      <a-spin :loading="logLoading" style="width: 100%">
        <a-empty v-if="logs.length === 0" :description="t('log.empty')" />
        <OperationLogList v-else :lines="logs" />
      </a-spin>
    </a-scrollbar>
  </a-modal>
</template>
