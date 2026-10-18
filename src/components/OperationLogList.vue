<script setup lang="ts">
/**
 * 操作日志列表：逐条分开显示。
 *
 * 「备份与日志」的日志页签与「日志管理」窗口共用这一份：两处显示的是同一份
 * 数据，各写一遍迟早会长歪——原先两处各有一份一模一样的模板。
 *
 * 每条日志拆成「时间 / 动作 / 细节」三段（见 utils/operation_log.ts）：
 * 时间戳单独一列并格式化成日期，动作单独成标签，细节另起一段换行。
 * 一条日志里的文件名可能很长，但边界清楚，逐条阅读不再吃力。
 */
import { computed } from "vue";
import { t } from "../i18n";
import { useAppStore } from "../store";
import { parseLogLine } from "../utils/operation_log";

const props = defineProps<{ lines: string[] }>();

const { formatTime } = useAppStore();

const entries = computed(() => props.lines.map(parseLogLine));

/** 时间戳解析不出来时如实说明，而不是显示成 1970 年 */
function timeText(timeNs: string): string {
  return timeNs ? formatTime(timeNs) : t("logList.timeUnknown");
}
</script>

<template>
  <div class="log-list">
    <div v-for="(e, i) in entries" :key="i" class="log-list__row">
      <span class="log-list__time">{{ timeText(e.timeNs) }}</span>
      <div class="log-list__body">
        <span v-if="e.action" class="log-list__action">{{ e.action }}</span>
        <span v-if="e.detail" class="log-list__detail">{{ e.detail }}</span>
      </div>
    </div>
  </div>
</template>
