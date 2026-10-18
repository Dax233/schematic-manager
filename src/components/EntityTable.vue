/**
 * 实体统计表（详情面板「实体」页签的内容）。
 *
 * 状态在 store/scenePreview.ts；实体坐标只如实汇报数量，
 * 预览渲染留待后续功能。
 *
 * 名字列与方块表同理：后端给出中英两套显示名称，按当前界面语言取一套。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { displayName, isFallbackName } from "../i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

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
      <template #display="{ record }">
        <span
          v-if="displayName(record)"
          :class="{ 'hint-dim': isFallbackName(record) }"
          :title="isFallbackName(record) ? t('displayName.missing') : undefined"
        >
          {{ displayName(record) }}
        </span>
        <span v-else class="hint-dim" :title="t('displayName.missing')">—</span>
      </template>
    </a-table>
    <!-- 坐标已随分析一起读出并存入，供以后做实体预览。
         这里如实说明读到了多少个，而不是让用户以为只有统计数字。 -->
    <div v-if="analysis.entities.length > 0" class="hint-dim" style="margin-top: 6px">
      {{ t("stats.entityInstances", { count: analysis.entityInstances.length }) }}
    </div>
    <a-empty
      v-if="analysis.entities.length === 0"
      :description="t('stats.noEntities')"
    />
  </template>
  <a-empty v-else :description="t('stats.noEntitiesSelected')" />
</template>
