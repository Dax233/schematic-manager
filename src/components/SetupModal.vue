/**
 * 版本库弹窗：选择目录（含子目录）递归扫描其中的客户端 jar 并自动导入，
 * 也可以勾选清单里没有的版本，经官方接口或 BMCL 镜像联网下载后构建
 * （方块库 + 实体贴图库）。
 *
 * 基础库不随安装包分发，首次启动库里为空时本界面会自动弹出引导；
 * 之后从工具栏「版本库」按钮再次打开，补充缺失的版本。
 * 状态与动作都在 store/setup.ts，这里只做展示与触发。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

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
  rescan,
  pickMcDir,
  selectSource,
  startSetup,
} = useAppStore();

/** 逐版本进度/结果的展示文本 */
function resultText(name: string): string {
  const o = outcomes.value.find((x) => x.id === name);
  if (o) return o.ok ? "已就绪" : o.error;
  const p = progress.value.get(name);
  if (!p) return "";
  if (p.stage === "download") {
    return p.total > 0
      ? `下载中 ${Math.floor((p.received / p.total) * 100)}%`
      : "下载中…";
  }
  if (p.stage === "build") return "构建中…";
  return "";
}

/** 下载进度百分比（0–100，Arco 进度条用的就是这个量纲）；
 *  构建阶段没有总量可报，返回 null 让进度条走不定态 */
function percentOf(name: string): number | null {
  const p = progress.value.get(name);
  if (!p || p.stage !== "download" || p.total <= 0) return null;
  return Math.round((p.received / p.total) * 100);
}
</script>

<template>
  <a-modal
    v-model:visible="setupOpen"
    title="版本库"
    :width="820"
    :footer="false"
    :closable="!running"
    :mask-closable="!running"
    unmount-on-close
  >
    <div class="hint-dim" style="margin-bottom: 12px">
      扫描本机该目录下所有可能的资源文件自动导入。也可选择官方接口或BMCL镜像联网下载。
      <template v-if="!entityAvailable && ready.length > 0">
        当前实体贴图库还没建起来，实体预览会缺贴图——重新准备任一版本即可补上。
      </template>
    </div>

    <a-space style="margin-bottom: 12px" wrap>
      <a-input
        v-model="mcDir"
        size="small"
        style="width: 360px"
        placeholder="目录路径（含子目录）"
        :disabled="running"
      />
      <a-button size="small" :disabled="running" @click="pickMcDir">浏览…</a-button>
      <a-button size="small" :loading="scanning" :disabled="running" @click="rescan()">
        扫描并导入
      </a-button>
      <a-radio-group
        :model-value="source"
        type="button"
        size="small"
        :disabled="running"
        @change="(v: any) => selectSource(v)"
      >
        <a-radio value="official">官方接口</a-radio>
        <a-radio value="bmcl">BMCL 镜像</a-radio>
      </a-radio-group>
      <a-button
        size="small"
        type="primary"
        :loading="running"
        :disabled="checked.size === 0"
        @click="startSetup"
      >
        准备勾选的版本（{{ checked.size }}）
      </a-button>
    </a-space>

    <div class="hint-dim" style="margin-bottom: 10px">
      「官方接口」从 Mojang 下载，国内网络常不通；连不上时换「BMCL 镜像」。
      本机扫到的版本直接导入、不消耗下载流量；勾选框用于清单里没有、要联网下载的版本。
    </div>

    <a-alert v-if="scanWarnings.length > 0" type="warning" style="margin-bottom: 10px">
      <template #title>
        扫描时有 {{ scanWarnings.length }} 项读不出版本信息，已跳过
      </template>
      <div v-for="w in scanWarnings" :key="w" class="dup-path">{{ w }}</div>
    </a-alert>

    <a-alert v-if="rows.length === 0" type="normal">正在读取版本库状态…</a-alert>
    <a-table
      v-else
      class="setup-table"
      :columns="setupColumns"
      :data="rows"
      :pagination="{ pageSize: 10 }"
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
        <a-tag v-if="record.state === 'ready'" size="small" color="green">已就绪</a-tag>
        <a-tag v-else-if="record.state === 'local'" size="small" color="arcoblue">
          本机已装
        </a-tag>
        <a-tag v-else size="small" color="orange">需下载</a-tag>
      </template>
      <template #result="{ record }">
        <template v-if="progress.has(record.name) || outcomes.some((o) => o.id === record.name)">
          <a-progress
            v-if="percentOf(record.name) !== null"
            size="small"
            :percent="percentOf(record.name)!"
            style="width: 160px; display: inline-block; margin-right: 8px"
          />
          <span :class="{ 'hint-dim': !outcomes.some((o) => o.id === record.name && !o.ok) }">
            {{ resultText(record.name) }}
          </span>
        </template>
      </template>
    </a-table>
  </a-modal>
</template>
