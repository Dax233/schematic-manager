/**
 * 详情面板：3D 预览（含视角/亮度/导出工具条）、可编辑元数据表单、
 * 「信息 / 方块 / 实体」三个页签。
 *
 * 它是右侧整块的容器；方块与实体两个页签的表格各自独立成
 * BlockStatTable / EntityTable，这里只留页签骨架。
 * 状态在 store/scenePreview.ts 与 store/fileList.ts（元数据草稿）。
 */
<script setup lang="ts">
import SceneViewer from "./SceneViewer.vue";
import BlockStatTable from "./BlockStatTable.vue";
import EntityTable from "./EntityTable.vue";
import { EXPORT_SIZES } from "../render/scene3d";
import { useAppStore } from "../store";
import { formatDims, formatSize } from "../utils/format";

const {
  viewer,
  scene,
  sceneLoading,
  sceneError,
  sceneInfo,
  brightness,
  activeView,
  exportSize,
  exporting,
  exportingProgress,
  setView,
  exportPreview,
  exportBatch,
  selected,
  draft,
  dirty,
  saving,
  resetDraft,
  saveDraft,
  checkedCount,
  hasChecked,
} = useAppStore();
</script>

<template>
  <div class="detail">
    <div class="preview">
      <!-- 预览与信息并排：左边是 3D 视图，右边是该投影的基本信息与元数据。
           用横向 flex 让「信息在预览右侧」这条要求不随窗口宽度漂移。 -->
      <div class="preview__main">
        <div class="preview__stage">
          <a-spin :loading="sceneLoading" style="width: 100%; height: 100%">
            <SceneViewer
              ref="viewer"
              :scene="scene"
              :brightness="brightness"
            />
          </a-spin>
        </div>
        <div class="preview__info" v-if="selected">
          <!-- 可编辑区：文件名、投影名称、作者、描述。操作行在它外面，
               因此内容再长也不会把保存按钮挤出可视范围。 -->
          <div class="preview__info-body">
          <!-- 这一栏只放四件事：文件名、投影名称、作者、描述。四者都可编辑。
               前三项按用户要求「名称在左、控件在右」横排；描述是长文本，
               标签留在上方、输入框占满整行。
               制作版本/数据版本/投影格式/尺寸/区域数量/方块总数/实体数量
               都已移到底部的「信息」页签，这里不再重复显示。 -->
          <!-- 文件名可改：改的是磁盘上的文件名，保存时会连同备份一起
               重新链接到新名字（见后端 rename_schematic）。
               输入框只放文件名本身，不含所在目录。 -->
          <div class="preview__row">
            <span class="preview__row-label">文件名</span>
            <a-input
              class="preview__row-control"
              v-model="draft.fileName"
              :disabled="!!selected.error"
              @input="dirty = true"
            />
          </div>
          <div class="hint-dim" style="margin: -6px 0 10px 66px">
            改名后保存即生效，所属目录不变
          </div>

          <a-form :model="draft" layout="vertical" size="small" class="preview__form">
            <div class="preview__row">
              <span class="preview__row-label">投影名称</span>
              <a-input
                class="preview__row-control"
                v-model="draft.name"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </div>
            <div class="preview__row">
              <span class="preview__row-label">作者</span>
              <a-input
                class="preview__row-control"
                v-model="draft.author"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </div>
            <a-form-item label="描述（备注）">
              <a-textarea
                v-model="draft.description"
                :auto-size="{ minRows: 6, maxRows: 14 }"
                placeholder="可多行填写，保存后写入投影文件"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </a-form-item>
            </a-form>
          </div>
          <!-- 操作行钉在面板底部，不随内容一起滚。
               面板高度是固定的（$preview-row-h），不钉住的话保存按钮会
               落到折叠线以下，用户得先滚动才能保存，等于按钮藏起来了。 -->
          <div class="preview__actions">
            <a-button
              type="primary"
              size="small"
              :loading="saving"
              :disabled="!dirty || !!selected.error"
              @click="saveDraft"
            >
              保存
            </a-button>
            <a-button size="small" :disabled="!dirty" @click="resetDraft(selected)">
              撤销
            </a-button>
            <span v-if="dirty" class="hint-warn">有未保存的修改</span>
            <span v-else-if="!selected.error" class="hint-dim">
              保存前会自动备份原件
            </span>
          </div>
        </div>
        <div class="preview__info" v-else>
          <a-empty description="选择一个投影文件以查看详情" />
        </div>
      </div>
      <div class="preview__bar">
        <a-radio-group
          v-model="activeView"
          type="button"
          size="small"
          :disabled="!scene"
          @change="setView(String(activeView))"
        >
          <a-radio value="front">正面</a-radio>
          <a-radio value="left45">左侧 45°</a-radio>
          <a-radio value="right45">右侧 45°</a-radio>
          <a-radio value="back">背面</a-radio>
        </a-radio-group>
        <span class="preview__label">亮度</span>
        <a-slider
          v-model="brightness"
          :min="0.5"
          :max="2"
          :step="0.05"
          :style="{ width: '140px' }"
        />
        <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
        <!-- 导出尺寸：四档可选，单个与批量共用。
             选项来自 EXPORT_SIZES，避免与 scene3d.ts 的定义各写一份而漂移 -->
        <a-select v-model="exportSize" size="small" style="width: 104px">
          <a-option v-for="s in EXPORT_SIZES" :key="s" :value="s">{{ s }}</a-option>
        </a-select>
        <a-button
          size="small"
          type="primary"
          :loading="exporting && !exportingProgress"
          :disabled="!scene || exporting"
          @click="exportPreview"
        >
          导出预览图
        </a-button>
        <!-- 批量导出：对目录栏里勾选的文件各导出一张，输出到所选目录 -->
        <a-button
          size="small"
          :loading="exporting && !!exportingProgress"
          :disabled="!hasChecked || exporting"
          :title="
            hasChecked
              ? `导出勾选的 ${checkedCount} 个投影各一张预览图`
              : '先在下方的目录栏里勾选要导出的投影'
          "
          @click="exportBatch"
        >
          批量导出
          <template v-if="exportingProgress">
            （{{ exportingProgress.done }}/{{ exportingProgress.total }}）
          </template>
          <template v-else-if="hasChecked">（{{ checkedCount }}）</template>
        </a-button>
        <span class="preview__spacer" />
        <span v-if="sceneError" class="preview__error">{{ sceneError }}</span>
        <span v-else-if="sceneInfo" class="preview__info">{{ sceneInfo }}</span>
      </div>
    </div>
    <a-tabs
      class="detail__tabs"
      default-active-key="info"
      style="padding: 0 8px"
    >
      <a-tab-pane key="info" title="信息">
        <!-- 只读明细。可编辑的元数据（文件名/投影名称/作者/描述）在预览区
             右侧，这里集中放**其余**的详细信息，避免两处重复同一字段。
             制作版本与数据版本分成两项：用户明确列了这两项，
             合并成「1.20.1（数据版本 3465）」会少一个可扫的标签。 -->
        <a-descriptions
          v-if="selected"
          :column="1"
          size="small"
          :label-style="{ width: '110px' }"
          bordered
        >
          <a-descriptions-item label="制作版本">
            {{ selected.mcVersionName || "未知" }}
          </a-descriptions-item>
          <a-descriptions-item label="数据版本">
            {{ selected.dataVersion }}
          </a-descriptions-item>
          <a-descriptions-item label="投影格式">v{{ selected.formatVersion }}</a-descriptions-item>
          <a-descriptions-item label="尺寸">{{ formatDims(selected.enclosing) }}</a-descriptions-item>
          <a-descriptions-item label="区域数量">{{ selected.regionCount }}</a-descriptions-item>
          <a-descriptions-item label="方块总数">{{ selected.totalBlocks }}</a-descriptions-item>
          <a-descriptions-item label="实体数量">{{ selected.entityCount }}</a-descriptions-item>
          <a-descriptions-item label="方块实体">{{ selected.tileEntityCount }}</a-descriptions-item>
          <a-descriptions-item label="待处理刻">{{ selected.pendingTicks }}</a-descriptions-item>
          <a-descriptions-item label="内含预览图">
            {{ selected.hasPreview ? "是" : "否" }}
          </a-descriptions-item>
          <a-descriptions-item label="文件大小">{{ formatSize(selected.fileSize) }}</a-descriptions-item>
          <a-descriptions-item label="所在目录">{{ selected.dirName }}</a-descriptions-item>
          <a-descriptions-item v-if="selected.error" label="解析错误">
            <span style="color: rgb(var(--danger-6))">{{ selected.error }}</span>
          </a-descriptions-item>
        </a-descriptions>
        <a-empty v-else description="选择一个投影文件以查看详情" />

      </a-tab-pane>

      <a-tab-pane key="blocks" title="方块">
        <BlockStatTable />
      </a-tab-pane>

      <a-tab-pane key="entities" title="实体">
        <EntityTable />
      </a-tab-pane>
    </a-tabs>
  </div>
</template>
