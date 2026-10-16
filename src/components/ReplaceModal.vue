/**
 * 方块替换弹窗。
 *
 * 向导式流程：填替换规则（源方块、属性限定、目标方块）→ 预览数量 → 执行。
 * 状态与流程都在 store/blockReplace.ts，这里只做展示与触发。
 *
 * 布局分左右两栏：左栏填规则，右栏是方块列表（搜索后点选即填入当前的
 * 那一栏）。两件事必须同时可见——填规则时要能随手挑方块，挑的时候又要
 * 看得见已经填了什么，挤在一个窄列里只能来回滚动，因此弹窗明显加宽。
 *
 * 凡是出现方块 ID 的地方都一并给出中文名：只给 ID 时用户无法确认自己
 * 要替换掉的、要换成的到底是哪一个方块。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

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
  blockZhOf,
  blockZhQueried,
  onBlockSearch,
  setPickField,
  pickBlock,
  doPreview,
  doReplace,
} = useAppStore();

/** 输入框里填的方块的中文名；没有记录时返回空串 */
function zhName(raw: string): string {
  return blockZhOf(raw.trim());
}

/** 还没查到 / 库里没有时的说明。
 *
 * 已经有名字时返回空串（那时走 zhName）：名字是内容，这句话是状态，
 * 两者在界面上的分量不同——名字要一眼看得清，状态只需说明情况。
 */
function zhStatus(raw: string): string {
  const id = raw.trim();
  if (!id || blockZhOf(id)) return "";
  return blockZhQueried(id)
    ? "方块库中未找到该 ID（或它没有中文名），请检查拼写"
    : "正在查询中文名…";
}
</script>

<template>
  <a-modal
    v-model:visible="replaceOpen"
    title="方块替换"
    :width="1180"
    :ok-loading="replaceBusy"
    ok-text="执行替换"
    :ok-button-props="{ disabled: !selected }"
    @ok="doReplace"
    modal-class="repl-window"
  >
    <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
      请先在左侧选择一个投影文件
    </a-alert>

    <div class="repl-split">
      <!-- 左栏：替换规则 -->
      <div class="repl-split__pane repl-split__pane--form">
        <a-form :model="replaceForm" layout="vertical" size="small">
          <!-- 每个字段一层 .repl-field：标签、输入框、说明一律纵向排开、间距一致。
               直接把输入框与说明并列写在表单项里时，Arco 的控制区会把它们摆成
               一行，说明文字跑到输入框右边，左侧就对不齐了。 -->
          <a-form-item label="源方块（要替换掉的）">
            <div class="repl-field">
              <!-- 还空着的必填输入框给一圈红边（见 .repl-block--empty），
                   填上就不上色：颜色只在需要提示时才出现 -->
              <div
                class="repl-block"
                :class="{ 'repl-block--empty': !replaceForm.fromName.trim() }"
              >
                <a-input
                  v-model="replaceForm.fromName"
                  placeholder="例如 minecraft:oak_stairs"
                  allow-clear
                  @focus="setPickField('from')"
                  @click="setPickField('from')"
                />
              </div>
              <div v-if="zhName(replaceForm.fromName)" class="repl-block__zh">
                中文名：{{ zhName(replaceForm.fromName) }}
              </div>
              <div v-else-if="zhStatus(replaceForm.fromName)" class="hint-dim">
                {{ zhStatus(replaceForm.fromName) }}
              </div>
              <div class="hint-dim">
                在「方块」页签点击任意行可直接填入，属性限定也会一并带入
              </div>
            </div>
          </a-form-item>

          <a-form-item label="源属性限定（可留空）">
            <div class="repl-field">
              <a-input
                v-model="replaceForm.fromPropsText"
                placeholder="例如 facing=east,half=top"
                allow-clear
              />
            </div>
          </a-form-item>

          <a-form-item label="目标方块（要换成的）">
            <div class="repl-field">
              <div
                class="repl-block"
                :class="{ 'repl-block--empty': !replaceForm.toName.trim() }"
              >
                <a-input
                  v-model="replaceForm.toName"
                  placeholder="例如 minecraft:stone"
                  allow-clear
                  @focus="setPickField('to')"
                  @click="setPickField('to')"
                />
              </div>
              <div v-if="zhName(replaceForm.toName)" class="repl-block__zh">
                中文名：{{ zhName(replaceForm.toName) }}
              </div>
              <div v-else-if="zhStatus(replaceForm.toName)" class="hint-dim">
                {{ zhStatus(replaceForm.toName) }}
              </div>
            </div>
          </a-form-item>

          <a-form-item label="目标属性覆盖（可选）">
            <div class="repl-field">
              <a-input
                v-model="replaceForm.toPropsText"
                placeholder="例如 facing=west"
                allow-clear
              />
            </div>
          </a-form-item>

          <a-form-item label="属性处理">
            <div class="repl-field">
              <a-checkbox v-model="replaceForm.inheritProps">
                继承源方块中目标也支持的属性
              </a-checkbox>
            </div>
          </a-form-item>

          <a-form-item>
            <div class="repl-field repl-field--row">
              <a-button :loading="replaceBusy" :disabled="!selected" @click="doPreview">
                预计替换数量
              </a-button>
              <span v-if="replacePreview !== null" class="repl-preview">
                将替换 <b>{{ replacePreview.toLocaleString() }}</b> 个方块
              </span>
            </div>
          </a-form-item>

          <div class="hint-dim">
            方块库共 {{ blockVersions.length }} 个版本可选；替换前会自动备份，
            可在「备份与日志」中还原。
          </div>
        </a-form>
      </div>

      <!-- 右栏：方块列表，点选即填到左栏选定的输入框 -->
      <div class="repl-split__pane repl-split__pane--pick">
        <a-input-search
          v-model="blockPickQuery"
          placeholder="搜索方块：中文名 / 英文名 / ID"
          size="small"
          :loading="blockSearching"
          allow-clear
          @input="onBlockSearch"
          @search="onBlockSearch"
          @clear="onBlockSearch('')"
        />
        <div class="hint-dim" style="margin: 6px 0">
          点下面的方块，填入
          <b>{{ replacePickField === "from" ? "源方块" : "目标方块" }}</b>
          （点左侧哪个输入框就填哪个）
        </div>
        <div class="repl-pick">
          <a-empty
            v-if="blockHits.length === 0"
            :description="blockPickQuery.trim() ? '没有匹配的方块' : '输入关键字搜索方块'"
          />
          <button
            v-for="h in blockHits"
            :key="h.name"
            type="button"
            class="repl-pick__item"
            :title="`点一下填入「${replacePickField === 'from' ? '源方块' : '目标方块'}」`"
            @click="pickBlock(h)"
          >
            <!-- 中文名在上、方块 ID 与英文名在下：中文名是用户认方块的
                 主要依据，ID 与英文名是精确依据 -->
            <span class="repl-block__zh">{{ h.zh || h.en || h.name }}</span>
            <span class="repl-block__id">
              {{ h.name }}<template v-if="h.en"> · {{ h.en }}</template>
              <template v-if="h.versions.length > 1">
                · {{ h.versions.length }} 个版本
              </template>
            </span>
          </button>
        </div>
      </div>
    </div>
  </a-modal>
</template>
