<script setup lang="ts">
import { computed, ref, watch, onBeforeUnmount } from "vue";
import { useI18n } from "vue-i18n";
import { containerItems } from "../api";
import { displayName } from "../i18n";
import { useAppStore } from "../store";
import type { ContainerItem } from "../types";

const { t } = useI18n();
const { selected, saving, activeTabId } = useAppStore();
const open = ref(false);
const loading = ref(false);
const error = ref("");
const rows = ref<ContainerItem[]>([]);
const query = ref("");
let request = 0;
function close() { ++request; open.value = false; loading.value = false; }
watch([() => selected.value?.path, activeTabId], close, {flush: "sync"});
onBeforeUnmount(close);
const filtered = computed(() => rows.value.filter(row => `${row.name} ${row.zhName} ${row.enName}`.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase())));
const columns = computed(() => [
  {title: t("inventory.name"), slotName: "name"},
  {title: t("inventory.count"), dataIndex: "count", width: 130},
  {title: t("inventory.boxes"), slotName: "boxes", width: 150},
]);
async function show() {
  const path = selected.value?.path;
  if (!path) return;
  const current = ++request;
  open.value = true; loading.value = true; error.value = ""; rows.value = []; query.value = "";
  try {
    const result = await containerItems(path);
    if (request === current) rows.value = result;
  } catch (e) {
    if (request === current) error.value = String(e);
  } finally {
    if (request === current) loading.value = false;
  }
}
</script>

<template>
  <a-button size="small" :disabled="!selected || !!selected.error || saving" @click="show">{{ t("inventory.title") }}</a-button>
  <a-modal :visible="open" :title="t('inventory.title')" :width="820" :footer="false" @cancel="close">
    <p>{{ selected?.fileName }}</p>
    <p class="hint-dim">{{ t("inventory.hint") }}</p>
    <a-input-search v-model="query" :placeholder="t('inventory.search')" allow-clear />
    <a-alert v-if="error" type="error" style="margin-top: 12px">{{ error }}</a-alert>
    <a-spin :loading="loading" style="width: 100%">
      <a-scrollbar style="max-height: 55vh; overflow: auto; margin-top: 12px">
        <a-table :columns="columns" :data="filtered" row-key="name" size="small" :pagination="{pageSize: 20}" :bordered="false">
          <template #name="{ record }">
            <div>{{ displayName(record) || record.name }}</div>
            <small class="hint-dim" v-if="displayName(record)">{{ record.name }}</small>
          </template>
          <template #boxes="{ record }">
            <span :title="record.boxes === null ? t('inventory.unknown') : t('inventory.capacity')">{{ record.boxes === null ? '—' : Number(record.boxes.toFixed(6)).toLocaleString(undefined, {maximumFractionDigits: 6}) }}</span>
          </template>
          <template #empty>{{ t('inventory.empty') }}</template>
        </a-table>
      </a-scrollbar>
    </a-spin>
  </a-modal>
</template>
