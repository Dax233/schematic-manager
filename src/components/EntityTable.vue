/**
 * 实体统计表（详情面板「实体」页签的内容）。
 *
 * 状态在 store/scenePreview.ts；实体坐标只如实汇报数量，
 * 预览渲染留待后续功能。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

const { analysis, entityColumns } = useAppStore();
</script>

<template>
  <template v-if="analysis">
    <a-table
      :columns="entityColumns"
      :data="analysis.entities"
      :pagination="false"
      row-key="id"
      size="small"
    >
      <template #zhName="{ record }">
        <span v-if="record.zhName">{{ record.zhName }}</span>
        <span v-else class="hint-dim" :title="record.enName || '语言表中没有这一项'">
          {{ record.enName || "—" }}
        </span>
      </template>
    </a-table>
    <!-- 坐标已随分析一起读出并存入，供以后做实体预览。
         这里如实说明读到了多少个，而不是让用户以为只有统计数字。 -->
    <div v-if="analysis.entities.length > 0" class="hint-dim" style="margin-top: 6px">
      已记录 {{ analysis.entityInstances.length }} 个实体的坐标（含区域原点），
      供后续实体预览使用
    </div>
    <a-empty
      v-if="analysis.entities.length === 0"
      description="该投影不含实体"
    />
  </template>
  <a-empty v-else description="选择投影文件后显示实体统计" />
</template>
