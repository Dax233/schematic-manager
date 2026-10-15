/**
 * 方块替换弹窗。
 *
 * 向导式流程：填替换规则（源方块、属性限定、目标方块）→ 预览数量 → 执行。
 * 状态与流程都在 store/blockReplace.ts，这里只做展示与触发。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

const {
  selected,
  replaceOpen,
  replaceBusy,
  blockHits,
  blockSearching,
  blockVersions,
  replaceForm,
  replacePreview,
  fromZh,
  fromZhState,
  onBlockSearch,
  doPreview,
  doReplace,
} = useAppStore();
</script>

<template>
  <a-modal
    v-model:visible="replaceOpen"
    title="方块替换"
    :width="640"
    :ok-loading="replaceBusy"
    ok-text="执行替换"
    :ok-button-props="{ disabled: !selected }"
    @ok="doReplace"
  >
    <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
      请先在左侧选择一个投影文件
    </a-alert>
    <a-form :model="replaceForm" layout="vertical" size="small">
      <a-form-item label="源方块（要替换掉的）">
        <a-input v-model="replaceForm.fromName" placeholder="例如 minecraft:oak_stairs" allow-clear />
        <!-- 源方块的中文名与目标方块一样要显示：用户从「方块」页签点行填入的
             是方块 ID，只有 ID 时无法确认自己点中的到底是哪一个 -->
        <div v-if="fromZh" class="hint-dim">中文名：{{ fromZh }}</div>
        <div v-else-if="fromZhState === 'loading'" class="hint-dim">正在查询中文名…</div>
        <div v-else-if="fromZhState === 'done' && replaceForm.fromName.trim()" class="hint-dim">
          方块库中未找到该 ID，请检查拼写
        </div>
        <div class="hint-dim">
          在「方块」页签点击任意行可直接填入，属性限定也会一并带入
        </div>
      </a-form-item>
      <a-form-item label="源属性限定（可留空）">
        <a-input
          v-model="replaceForm.fromPropsText"
          placeholder="例如 facing=east,half=top"
          allow-clear
        />
      </a-form-item>

      <a-form-item label="目标方块">
        <a-select
          v-model="replaceForm.toName"
          placeholder="输入中文名、英文名或方块 ID 搜索"
          allow-search
          allow-clear
          :filter-option="false"
          :loading="blockSearching"
          @search="onBlockSearch"
        >
          <a-option v-for="h in blockHits" :key="h.name" :value="h.name">
            <span>{{ h.zh || h.en || h.name }}</span>
            <span class="hint-dim" style="margin-left: 8px">{{ h.name }}</span>
            <span v-if="h.versions.length > 1" class="hint-dim" style="margin-left: 8px">
              {{ h.versions.length }} 个版本
            </span>
          </a-option>
        </a-select>
      </a-form-item>
      <a-form-item label="目标属性覆盖（可选）">
        <a-input
          v-model="replaceForm.toPropsText"
          placeholder="例如 facing=west"
          allow-clear
        />
      </a-form-item>

      <a-form-item label="属性处理">
        <a-space direction="vertical" :size="4">
          <a-checkbox v-model="replaceForm.inheritProps">
            继承源方块中目标也支持的属性
          </a-checkbox>          </a-space>
      </a-form-item>

      <a-form-item>
        <a-space>
          <a-button :loading="replaceBusy" :disabled="!selected" @click="doPreview">
            预计替换数量
          </a-button>
          <span v-if="replacePreview !== null" class="preview__info">
            将替换 <b>{{ replacePreview.toLocaleString() }}</b> 个方块
          </span>
        </a-space>
      </a-form-item>
      <div class="hint-dim">
        方块库共 {{ blockVersions.length }} 个版本可选；替换前会自动备份，可在「备份管理」中还原。
      </div>
    </a-form>
  </a-modal>
</template>
