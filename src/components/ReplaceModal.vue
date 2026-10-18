/**
 * 方块替换弹窗。
 *
 * 向导式流程：填替换规则（源方块、属性限定、目标方块）→ 预览数量 → 执行。
 * 状态与流程都在 store/blockReplace.ts，这里只做展示与触发。
 *
 * 布局分左右两栏：左栏填规则，右栏是方块列表（搜索后点选即填入当前的
 * 那一栏）。两件事必须同时可见——填规则时要能随手挑方块，挑的时候又要
 * 看得见已经填了什么，挤在一个窄列里只能来回滚动，因此弹窗明显加宽。
 * 两栏各自用 Arco 的无轨道滚动条，超出的部分在栏内滚。
 *
 * 凡是出现方块 ID 的地方都一并给出显示名称：只给 ID 时用户无法确认自己
 * 要替换掉的、要换成的到底是哪一个方块。名字分中英两套（后端从 Minecraft
 * 语言表取出），按当前界面语言取一套。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { displayName } from "../i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  selected,
  replaceOpen,
  replaceBusy,
  blockPickQuery,
  blockHits,
  blockSearching,
  blockVersions,
  replacePickField,
  replaceForm,
  replacePreview,
  blockNameOf,
  blockNameQueried,
  onBlockSearch,
  setPickField,
  pickBlock,
  doPreview,
  doReplace,
} = useAppStore();

/** 输入框里填的方块的显示名称；没有记录时返回空串 */
function shownName(raw: string): string {
  return blockNameOf(raw.trim());
}

/** 还没查到 / 库里没有时的说明。
 *
 * 已经有名字时返回空串（那时走 shownName）：名字是内容，这句话是状态，
 * 两者在界面上的分量不同——名字要一眼看得清，状态只需说明情况。
 */
function nameStatus(raw: string): string {
  const id = raw.trim();
  if (!id || blockNameOf(id)) return "";
  return blockNameQueried(id) ? t("replace.notFound") : t("replace.querying");
}

/** 右侧列表点选会填到哪一栏 */
function pickFieldLabel(): string {
  return replacePickField.value === "from" ? t("replace.fieldFrom") : t("replace.fieldTo");
}
</script>

<template>
  <a-modal
    v-model:visible="replaceOpen"
    :title="t('replace.title')"
    :width="1180"
    :ok-loading="replaceBusy"
    :ok-text="t('replace.ok')"
    :ok-button-props="{ disabled: !selected }"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    @ok="doReplace"
    modal-class="repl-window"
  >
    <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
      {{ t("replace.pickFile") }}
    </a-alert>

    <div class="repl-split">
      <!-- 左栏：替换规则 -->
      <div class="repl-split__pane repl-split__pane--form">
        <a-scrollbar type="embed" :outer-style="{ flex: '1 1 auto', minHeight: 0 }">
          <a-form :model="replaceForm" layout="vertical" size="small">
            <!-- 每个字段一层 .repl-field：标签、输入框、说明一律纵向排开、间距一致。
                 直接把输入框与说明并列写在表单项里时，Arco 的控制区会把它们摆成
                 一行，说明文字跑到输入框右边，左侧就对不齐了。 -->
            <a-form-item :label="t('replace.fromLabel')">
              <div class="repl-field">
                <!-- 还空着的必填输入框给一圈红边（见 .repl-block--empty），
                     填上就不上色：颜色只在需要提示时才出现 -->
                <div
                  class="repl-block"
                  :class="{ 'repl-block--empty': !replaceForm.fromName.trim() }"
                >
                  <a-input
                    v-model="replaceForm.fromName"
                    :placeholder="t('replace.fromPlaceholder')"
                    allow-clear
                    @focus="setPickField('from')"
                    @click="setPickField('from')"
                  />
                </div>
                <div v-if="shownName(replaceForm.fromName)" class="repl-block__zh">
                  {{ t("replace.displayName", { name: shownName(replaceForm.fromName) }) }}
                </div>
                <div v-else-if="nameStatus(replaceForm.fromName)" class="hint-dim">
                  {{ nameStatus(replaceForm.fromName) }}
                </div>
                <div class="hint-dim">{{ t("replace.fromPickHint") }}</div>
              </div>
            </a-form-item>

            <a-form-item :label="t('replace.fromPropsLabel')">
              <div class="repl-field">
                <a-input
                  v-model="replaceForm.fromPropsText"
                  :placeholder="t('replace.fromPropsPlaceholder')"
                  allow-clear
                />
              </div>
            </a-form-item>

            <a-form-item :label="t('replace.toLabel')">
              <div class="repl-field">
                <div
                  class="repl-block"
                  :class="{ 'repl-block--empty': !replaceForm.toName.trim() }"
                >
                  <a-input
                    v-model="replaceForm.toName"
                    :placeholder="t('replace.toPlaceholder')"
                    allow-clear
                    @focus="setPickField('to')"
                    @click="setPickField('to')"
                  />
                </div>
                <div v-if="shownName(replaceForm.toName)" class="repl-block__zh">
                  {{ t("replace.displayName", { name: shownName(replaceForm.toName) }) }}
                </div>
                <div v-else-if="nameStatus(replaceForm.toName)" class="hint-dim">
                  {{ nameStatus(replaceForm.toName) }}
                </div>
              </div>
            </a-form-item>

            <a-form-item :label="t('replace.toPropsLabel')">
              <div class="repl-field">
                <a-input
                  v-model="replaceForm.toPropsText"
                  :placeholder="t('replace.toPropsPlaceholder')"
                  allow-clear
                />
              </div>
            </a-form-item>

            <a-form-item :label="t('replace.propsLabel')">
              <div class="repl-field">
                <a-checkbox v-model="replaceForm.inheritProps">
                  {{ t("replace.inheritProps") }}
                </a-checkbox>
              </div>
            </a-form-item>

            <a-form-item>
              <div class="repl-field repl-field--row">
                <a-button :loading="replaceBusy" :disabled="!selected" @click="doPreview">
                  {{ t("replace.previewCount") }}
                </a-button>
                <!-- 数字要加粗，而句式随语言变化，因此整句走 i18n-t -->
                <i18n-t scope="global"
                  v-if="replacePreview !== null"
                  keypath="replace.previewResult"
                  tag="span"
                  class="repl-preview"
                >
                  <template #count>
                    <b>{{ replacePreview.toLocaleString() }}</b>
                  </template>
                </i18n-t>
              </div>
            </a-form-item>

            <div class="hint-dim">
              {{ t("replace.libraryHint", { count: blockVersions.length }) }}
            </div>
          </a-form>
        </a-scrollbar>
      </div>

      <!-- 右栏：方块列表，点选即填到左栏选定的输入框 -->
      <div class="repl-split__pane repl-split__pane--pick">
        <a-input-search
          v-model="blockPickQuery"
          :placeholder="t('common.searchBlocks')"
          size="small"
          :loading="blockSearching"
          allow-clear
          @input="onBlockSearch"
          @search="onBlockSearch"
          @clear="onBlockSearch('')"
        />
        <div class="hint-dim" style="margin: 6px 0">
          <i18n-t scope="global" keypath="replace.pickHint" tag="span">
            <template #field><b>{{ pickFieldLabel() }}</b></template>
          </i18n-t>
        </div>
        <a-scrollbar
          type="embed"
          class="repl-pick"
          :outer-style="{ flex: '1 1 auto', minHeight: 0 }"
        >
          <a-empty
            v-if="blockHits.length === 0"
            :description="
              blockPickQuery.trim() ? t('common.noMatch') : t('common.typeToSearch')
            "
          />
          <button
            v-for="h in blockHits"
            :key="h.name"
            type="button"
            class="repl-pick__item"
            :title="t('replace.pickTitle', { field: pickFieldLabel() })"
            @click="pickBlock(h)"
          >
            <!-- 显示名称在上、方块 ID 与英文名在下：名字是用户认方块的
                 主要依据，ID 与英文名是精确依据。英文名与上面的显示名称
                 相同时不再重复给出（英语界面下本来就会重复一次） -->
            <span class="repl-block__zh">
              {{ displayName({ zhName: h.zh, enName: h.en }) || h.name }}
            </span>
            <span class="repl-block__id">
              {{ h.name }}<template v-if="h.en && h.en !== h.zh"> · {{ h.en }}</template>
              <template v-if="h.versions.length > 1">
                {{ t("replace.versionCount", { count: h.versions.length }) }}
              </template>
            </span>
          </button>
        </a-scrollbar>
      </div>
    </div>
  </a-modal>
</template>
