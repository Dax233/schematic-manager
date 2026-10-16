/**
 * 备份与日志抽屉：备份清单（还原/删除）与全局操作日志两个页签。
 *
 * 状态与动作都在 store/backups.ts，这里只做展示与触发。
 */
<script setup lang="ts">
import { useAppStore } from "../store";
import OperationLogList from "./OperationLogList.vue";
import { formatSize } from "../utils/format";

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
</script>

<template>
  <a-drawer
    v-model:visible="backupOpen"
    title="备份与日志"
    :width="620"
    :footer="false"
    unmount-on-close
  >
    <a-tabs default-active-key="backups">
      <a-tab-pane key="backups" :title="`备份（${backups.length}）`">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshBackups">刷新</a-button>
          <span v-if="selected" class="hint-dim">
            仅显示「{{ selected.fileName }}」的备份
          </span>
        </a-space>
        <a-table
          :columns="[
            { title: '时间', slotName: 'time', width: 170 },
            { title: '操作', dataIndex: 'op', width: 110 },
            { title: '大小', slotName: 'size', width: 90 },
            { title: '操作', slotName: 'act', width: 150 },
          ]"
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
              <a-link :disabled="!selected" @click="onRestore(record)">还原</a-link>
              <a-popconfirm content="删除后无法恢复，确认删除？" @ok="onDelete(record)">
                <a-link status="danger">删除</a-link>
              </a-popconfirm>
            </a-space>
          </template>
          <template #empty>
            <a-empty :description="selected ? '该文件暂无备份' : '请先选择一个投影文件'" />
          </template>
        </a-table>
      </a-tab-pane>

      <a-tab-pane key="logs" title="操作日志">
        <a-space style="margin-bottom: 8px">
          <a-button size="small" @click="refreshLogs">刷新</a-button>
          <!-- 日志管理窗口（清空、打开日志目录）从这里进：它原先挂在资源包
               管理窗口上，那个窗口已移除；日志属于备份与运维这条线，放在
               同一条操作日志页签里最顺手 -->
          <a-button size="small" @click="openLogs">日志管理</a-button>
        </a-space>
        <a-spin :loading="logLoading" style="width: 100%">
          <a-empty v-if="logs.length === 0" description="暂无日志" />
          <OperationLogList v-else :lines="logs" />
        </a-spin>
      </a-tab-pane>
    </a-tabs>
  </a-drawer>
</template>
