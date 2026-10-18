/**
 * 版本库弹窗：选择目录（含子目录）递归扫描其中的客户端 jar 并自动导入，
 * 也可以勾选清单里没有的版本，经官方接口或 BMCL 镜像联网下载后构建
 * （方块库 + 实体贴图库）。
 *
 * 基础库不随安装包分发，首次启动库里为空时本界面会自动弹出引导；
 * 之后从工具栏「版本库」按钮再次打开，补充缺失的版本。
 * 状态与动作都在 store/setup.ts，这里只做展示与触发。
 *
 * 列表不翻页：支持的正式版有几十个，翻页会让人漏看「已经就绪」的那些；
 * 整表放进弹窗内容区的滚动条里，位置全靠滚动，找起来比翻页直接。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  setupOpen,
  scanning,
  running,
  mcDir,
  source,
  scanWarnings,
  entityAvailable,
  ready,
  checked,
  progress,
  outcomes,
  rows,
  setupColumns,
  toggleChecked,
  allChecked,
  setAllChecked,
  rescan,
  pickMcDir,
  selectSource,
  startSetup,
} = useAppStore();

/** 下载进度比例（0～1）。构建阶段没有总量可报，返回 null 让进度条走不定态。
 *
 * 必须是比例而不是百分数：Arco 的 a-progress 内部自己会乘 100——web-vue 的
 * progress/line.js 里文本与条宽都是 percent * 100，progress.js 又把 percent
 * 原样透传下去，所以传百分数会显示成「900%」且条宽 900% 被轨道裁成满格。
 *
 * total 取自 Content-Length，服务器给的未必是正文真实长度，夹到 1 以内可以
 * 避免条宽超过 100% 溢出轨道。 */
function downloadProgress(name: string): number | null {
  const p = progress.value.get(name);
  if (!p || p.stage !== "download" || p.total <= 0) return null;
  return Math.min(1, p.received / p.total);
}

/** 逐版本进度/结果的展示文本。
 *
 * 下载阶段不在这里写百分比：进度条自己就带着 Arco 渲染的百分比（进度条右侧），
 * 再写一遍会变成「9% 下载中 9%」。只有画不出进度条（服务器没给长度）时才补一句。 */
function resultText(name: string): string {
  const o = outcomes.value.find((x) => x.id === name);
  if (o) return o.ok ? t("setup.ready") : o.error;
  const p = progress.value.get(name);
  if (!p) return "";
  if (p.stage === "download") {
    return downloadProgress(name) === null ? t("setup.progressUnknown") : "";
  }
  if (p.stage === "build") return t("setup.progressBuilding");
  return "";
}
</script>

<template>
  <a-modal
    v-model:visible="setupOpen"
    :title="t('setup.title')"
    :width="820"
    :footer="false"
    :closable="!running"
    :mask-closable="!running"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    unmount-on-close
  >
    <div class="hint-dim" style="margin-bottom: 12px">
      {{ t("setup.hint") }}
      <template v-if="!entityAvailable && ready.length > 0">
        {{ t("setup.entityMissing") }}
      </template>
    </div>

    <a-space style="margin-bottom: 12px" wrap>
      <a-input
        v-model="mcDir"
        size="small"
        style="width: 360px"
        :placeholder="t('setup.dirPlaceholder')"
        :disabled="running"
      />
      <a-button size="small" :disabled="running" @click="pickMcDir">
        {{ t("setup.browse") }}
      </a-button>
      <a-button size="small" :loading="scanning" :disabled="running" @click="rescan()">
        {{ t("setup.scan") }}
      </a-button>
      <a-radio-group
        :model-value="source"
        type="button"
        size="small"
        :disabled="running"
        @change="(v: any) => selectSource(v)"
      >
        <a-radio value="official">{{ t("setup.sourceOfficial") }}</a-radio>
        <a-radio value="bmcl">{{ t("setup.sourceBmcl") }}</a-radio>
      </a-radio-group>
      <a-button
        size="small"
        :disabled="running || rows.length === 0"
        @click="setAllChecked(!allChecked)"
      >
        {{ allChecked ? t("setup.uncheckAll") : t("setup.checkAll") }}
      </a-button>
      <a-button
        size="small"
        type="primary"
        :loading="running"
        :disabled="checked.size === 0"
        @click="startSetup"
      >
        {{ t("setup.startSetup", { count: checked.size }) }}
      </a-button>
    </a-space>

    <div class="hint-dim" style="margin-bottom: 10px">{{ t("setup.sourceHint") }}</div>

    <!-- 内容区自己滚：弹窗高度不铺满整屏，版本多的时候在这里滚 -->
    <a-scrollbar type="embed" :outer-style="{ flex: '1 1 auto', minHeight: 0 }">
      <a-alert v-if="scanWarnings.length > 0" type="warning" style="margin-bottom: 10px">
        <template #title>
          {{ t("setup.scanWarnings", { count: scanWarnings.length }) }}
        </template>
        <div v-for="w in scanWarnings" :key="w" class="dup-path">{{ w }}</div>
      </a-alert>

      <a-alert v-if="rows.length === 0" type="normal">
        {{ t("setup.loadingStatus") }}
      </a-alert>
      <a-table
        v-else
        class="setup-table"
        :columns="setupColumns"
        :data="rows"
        :pagination="false"
        size="small"
        row-key="name"
      >
        <template #check="{ record }">
          <!-- 已就绪的行也能勾：贴图库缺了或包损坏时要靠重做来恢复，
               构建本身是幂等的（同名文件覆盖、同数据版本替换） -->
          <a-checkbox
            :model-value="checked.has(record.name)"
            :disabled="running"
            @change="(v: any) => toggleChecked(record.name, Boolean(v))"
          />
        </template>
        <template #state="{ record }">
          <a-tag v-if="record.state === 'ready'" size="small" color="green">
            {{ t("setup.ready") }}
          </a-tag>
          <a-tag v-else-if="record.state === 'local'" size="small" color="arcoblue">
            {{ t("setup.local") }}
          </a-tag>
          <a-tag v-else size="small" color="orange">{{ t("setup.missing") }}</a-tag>
        </template>
        <template #result="{ record }">
          <template v-if="progress.has(record.name) || outcomes.some((o) => o.id === record.name)">
            <!-- 百分比由进度条自己显示（Arco 内置文本），这里不再重复写一遍 -->
            <a-progress
              v-if="downloadProgress(record.name) !== null"
              size="small"
              :percent="downloadProgress(record.name)!"
              style="width: 160px; display: inline-block; margin-right: 8px"
            />
            <span :class="{ 'hint-dim': !outcomes.some((o) => o.id === record.name && !o.ok) }">
              {{ resultText(record.name) }}
            </span>
          </template>
        </template>
      </a-table>
    </a-scrollbar>
  </a-modal>
</template>
