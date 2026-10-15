/**
 * 日志管理窗口：查看全局操作日志，并提供清空与定位日志文件两个管理动作。
 *
 * 与「备份管理」里的操作日志页签是同一份数据（store/backups.ts 的 logs），
 * 这里只是把它做成可以单独打开的窗口——入口在资源包管理窗口右侧，
 * 不必先绕进备份管理。
 *
 * 清空是不可撤销的，因此走 a-popconfirm 二次确认；日志文件本身保留
 * （见后端 backup::clear_log），所以「打开日志目录」在清空后仍然可用。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

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
    title="日志管理"
    :width="760"
    :footer="false"
    unmount-on-close
  >
    <!-- 只列后端真的会写日志的操作（commands.rs 里 append_log 的调用点）：
         说多了就是错误陈述，用户会照着这句话去找一条并不存在的记录。 -->
    <div class="hint-dim" style="margin-bottom: 8px">
      记录投影文件的改动：方块替换、版本转换、重命名、复制到目录、删除到回收站，
      以及备份的还原与删除。最新的在最上面，最多显示最近 {{ logMaxLines }} 条。
    </div>

    <a-space style="margin-bottom: 8px">
      <a-button size="small" :loading="logLoading" @click="refreshLogs">刷新</a-button>
      <a-button
        size="small"
        title="在文件管理器里打开日志所在目录，并选中日志文件"
        @click="openLogDir"
      >
        打开日志所在目录
      </a-button>
      <a-popconfirm
        content="清空后无法恢复，确定清空操作日志？"
        ok-text="清空"
        @ok="clearLogs"
      >
        <a-button size="small" status="danger" :disabled="logs.length === 0">
          清空日志
        </a-button>
      </a-popconfirm>
      <span class="hint-dim">共 {{ logs.length }} 条</span>
    </a-space>

    <a-spin :loading="logLoading" style="width: 100%">
      <a-empty v-if="logs.length === 0" description="暂无日志" />
      <div v-else class="log-list">
        <div v-for="(line, i) in logs" :key="i" class="log-list__row">{{ line }}</div>
      </div>
    </a-spin>
  </a-modal>
</template>
