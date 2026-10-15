/**
 * 资源包管理弹窗。
 *
 * 资源包清单的展示、导入（单个 jar / 整个 mods 文件夹）、启停与删除。
 * 清单损坏行的拦截提示也在这里——它是用户唯一能修复清单的入口。
 * 状态与动作都在 store/packs.ts，这里只做展示与触发。
 */
<script setup lang="ts">
import { useAppStore } from "../store";
import { formatSize } from "../utils/format";

const {
  packOpen,
  packBusy,
  packs,
  packSkipped,
  packLoadFailures,
  importKind,
  importDataVersion,
  importBusy,
  modsBusy,
  overlayVersions,
  packColumns,
  packPending,
  refreshPacks,
  openPacksDir,
  runImportJar,
  runImportModsFolder,
  togglePack,
  removePack,
} = useAppStore();
</script>

<template>
  <a-modal
    v-model:visible="packOpen"
    title="资源包管理"
    :width="900"
    :footer="false"
    unmount-on-close
  >
    <div class="hint-dim" style="margin-bottom: 12px">
      基础库（在「版本库」里准备的版本）已含各正式版的贴图，通常无需
      手动导入。只有在使用模组时，才需要导入 jar。
    </div>

    <a-space style="margin-bottom: 12px" wrap>
      <a-radio-group v-model="importKind" type="button" size="small">
        <a-radio :value="1">MC 版本包</a-radio>
        <a-radio :value="2">模组包</a-radio>
      </a-radio-group>
      <a-select
        v-if="importKind === 2"
        v-model="importDataVersion"
        size="small"
        style="width: 190px"
        placeholder="叠加到哪个版本"
      >
        <a-option
          v-for="v in overlayVersions"
          :key="v.index"
          :value="v.dataVersion"
        >
          {{ v.name }}
        </a-option>
      </a-select>
      <a-button size="small" type="primary" :loading="importBusy" @click="runImportJar">
        导入 jar…
      </a-button>
      <!-- 整目录导入：装了上百个模组时逐个选 jar 不可行 -->
      <a-button size="small" :loading="modsBusy" @click="runImportModsFolder">
        导入 mods 文件夹…
      </a-button>
      <a-button
        size="small"
        title="在文件管理器里打开资源包存放目录"
        @click="openPacksDir"
      >
        打开资源包目录
      </a-button>
      <a-button size="small" :loading="packBusy" @click="refreshPacks">刷新</a-button>
    </a-space>

    <div class="hint-dim" style="margin-bottom: 10px">
      <template v-if="importKind === 2">
        「导入 mods 文件夹」会一次导入该文件夹下<strong>所有</strong>模组 jar
        （只扫描这一层、不递归），并跳过客户端 jar；
        单个模组失败不会中断整批。
      </template>
      <template v-else>
        选具体的 .jar 文件而不是文件夹：
        <code>.minecraft\versions\&lt;版本&gt;\&lt;版本&gt;.jar</code>
        。要选客户端而不是服务端——服务端没有贴图资源。
      </template>
    </div>

    <a-alert
      v-if="packLoadFailures.length > 0"
      type="error"
      style="margin-bottom: 10px"
    >
      <template #title>
        {{ packLoadFailures.length }} 个已启用的资源包未能载入，它们不会生效
      </template>
      <div v-for="f in packLoadFailures" :key="f.file" class="dup-path">
        {{ f.file }}：{{ f.reason }}
      </div>
      <div style="margin-top: 6px">
        常见原因是文件被手工删除或包本身损坏；重新导入该 jar 即可修复。
      </div>
    </a-alert>

    <a-alert
      v-if="packSkipped.length > 0"
      type="warning"
      style="margin-bottom: 10px"
    >
      <template #title>
        清单有 {{ packSkipped.length }} 行无法解析，这些记录不会被加载
      </template>
      <div v-for="s in packSkipped" :key="s" class="dup-path">{{ s }}</div>
      <div style="margin-top: 6px">
        修好这些行之前，导入、启用与删除都会被拒绝——否则重写清单会把它们
        连同内容一起抹掉。清单位置：&lt;程序目录&gt;\packs\packs.tsv
      </div>
    </a-alert>

    <a-spin :loading="packBusy" style="width: 100%">
      <a-alert v-if="packs.length === 0 && packSkipped.length === 0" type="normal">
        尚未导入任何资源包
      </a-alert>
      <a-alert v-else-if="packs.length === 0" type="warning">
        清单里的记录全部无法解析，请按上方说明修复后再刷新
      </a-alert>
      <a-table
        v-else
        :columns="packColumns"
        :data="packs"
        :pagination="{ pageSize: 8 }"
        size="small"
        row-key="file"
      >
        <template #kind="{ record }">
          <a-tag size="small">{{ record.kindLabel }}</a-tag>
        </template>
        <template #version="{ record }">
          <span>{{ record.mcVersion || "-" }}</span>
        </template>
        <!-- 来源目录：旧清单没有这一列（升级前导入的包），显示占位而不是
             空白，否则用户会以为是界面坏了。 -->
        <template #sourceDir="{ record }">
          <span v-if="record.sourceDir" :title="record.sourceDir">
            {{ record.sourceDir }}
          </span>
          <span v-else class="hint-dim">（旧记录未保存）</span>
        </template>
        <template #size="{ record }">
          <span v-if="record.exists">{{ formatSize(record.size) }}</span>
          <a-tag v-else size="small" color="red">文件缺失</a-tag>
        </template>
        <template #state="{ record }">
          <a-switch
            size="small"
            :model-value="record.enabled"
            :loading="packPending.has(record.file)"
            @change="(v: any) => togglePack(record, Boolean(v))"
          />
        </template>
        <template #actions="{ record }">
          <a-popconfirm
            content="删除后需要重新导入才能恢复，确定删除？"
            @ok="removePack(record)"
          >
            <a-button
              size="mini"
              status="danger"
              :loading="packPending.has(record.file)"
            >
              删除
            </a-button>
          </a-popconfirm>
        </template>
      </a-table>
    </a-spin>
  </a-modal>
</template>
