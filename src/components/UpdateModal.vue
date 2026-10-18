/**
 * 更新窗口：显示当前版本、作者与作者主页，并可检测 GitHub 上有没有新版本。
 *
 * 版本号取自后端 `app_version`（编译进二进制的包版本），不是界面里的常量：
 * CI 在构建前才把版本号写入 Cargo.toml，界面若自己写一份，两者迟早对不上。
 *
 * 检测结果分四种，分别用不同措辞，避免把「没查到」说成「已是最新」：
 * 有更新 / 已是最新 / 仓库里没有可解析的发布 / 检测失败（附原因）。
 *
 * 版本性质（正式版／预览版）由后端给出——用的是发布链路判定 GitHub
 * `prerelease` 标记的同一条规则，界面不自己解析版本号，免得两处各说各话。
 * 只标出来、不改变检测逻辑：要不要跨性质更新由用户自己决定。
 *
 * 下载地址与作者主页都交给系统浏览器打开（后端 open_url）。下载地址框点击
 * 即复制——地址通常用来贴给别人，选中再按 Ctrl+C 是多一步。
 */
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  updateOpen,
  currentVersion,
  updateInfo,
  updateChecking,
  updateError,
  releaseUrl,
  authorPage,
  runUpdateCheck,
  copyReleaseUrl,
  openReleaseUrl,
  openAuthorPage,
} = useAppStore();

/** 作者信息：按用户要求展示 */
const AUTHOR = "bilibili黑山大叔";

/** 版本性质标签。橙色预览版、绿色正式版，与发布页上的性质一致。 */
const kindLabel = (prerelease: boolean) =>
  prerelease ? t("update.prerelease") : t("update.stable");
const kindColor = (prerelease: boolean) => (prerelease ? "orange" : "green");
</script>

<template>
  <a-modal
    v-model:visible="updateOpen"
    :title="t('update.title')"
    :width="600"
    :footer="false"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    unmount-on-close
  >
    <!-- 版本信息与检测结果都可以比窗口高：内容区自己滚，标题留在视野里 -->
    <a-scrollbar type="embed" style="flex: 1 1 auto; min-height: 0; overflow: auto">
      <div class="about-row">
        <span class="about-row__label">{{ t("update.currentVersion") }}</span>
        <span class="about-row__value">
          {{ currentVersion || t("update.unknown") }}
          <!-- 检测过才知道当前版本的性质；没检测时不猜 -->
          <a-tag
            v-if="updateInfo"
            :color="kindColor(updateInfo.currentPrerelease)"
            size="small"
            style="margin-left: 6px"
          >
            {{ kindLabel(updateInfo.currentPrerelease) }}
          </a-tag>
        </span>
      </div>
      <div class="about-row">
        <span class="about-row__label">{{ t("update.author") }}</span>
        <span class="about-row__value">{{ AUTHOR }}</span>
      </div>
      <div class="about-row">
        <span class="about-row__label">{{ t("update.authorPage") }}</span>
        <span class="about-row__value">
          <a-link class="about-link" @click="openAuthorPage">{{ authorPage }}</a-link>
        </span>
      </div>
      <div class="about-row">
        <span class="about-row__label">{{ t("update.checkTarget") }}</span>
        <span class="about-row__value">
          {{ updateInfo?.repo || "heiheids/schematic-manager" }}
        </span>
      </div>

      <a-space style="margin: 16px 0 8px">
        <a-button type="primary" size="small" :loading="updateChecking" @click="runUpdateCheck">
          {{ t("update.check") }}
        </a-button>
        <span class="hint-dim">{{ t("update.checkHint") }}</span>
      </a-space>

      <!-- 检测失败：把原因如实显示出来。这里最容易出错的表述是把网络失败
           说成「已是最新」，用户会以为确实没有新版本。 -->
      <a-alert v-if="updateError" type="error" style="margin-bottom: 8px">
        {{ updateError }}
      </a-alert>

      <template v-else-if="updateInfo">
        <a-alert v-if="updateInfo.hasUpdate" type="success" style="margin-bottom: 8px">
          {{ t("update.hasUpdate", { version: updateInfo.latest }) }}
          <a-tag
            :color="kindColor(updateInfo.latestPrerelease)"
            size="small"
            style="margin-left: 6px"
          >
            {{ kindLabel(updateInfo.latestPrerelease) }}
          </a-tag>
        </a-alert>
        <a-alert v-else-if="updateInfo.latest" type="info" style="margin-bottom: 8px">
          {{ t("update.upToDate", { version: updateInfo.latest }) }}
          <a-tag
            :color="kindColor(updateInfo.latestPrerelease)"
            size="small"
            style="margin-left: 6px"
          >
            {{ kindLabel(updateInfo.latestPrerelease) }}
          </a-tag>
        </a-alert>
        <!-- 查到了接口、但里面没有可解析的发布，这与「最新」是两回事 -->
        <a-alert v-else type="warning" style="margin-bottom: 8px">
          {{ t("update.noRelease") }}
        </a-alert>

        <div v-if="updateInfo.publishedAt" class="hint-dim" style="margin-bottom: 8px">
          {{ t("update.publishedAt", { time: updateInfo.publishedAt }) }}
        </div>
      </template>

      <div v-else-if="!updateError" class="hint-dim" style="margin-bottom: 8px">
        {{ t("update.idle") }}
      </div>

      <!-- 下载地址：未检测时也给出发布列表页，随时可打开或复制 -->
      <div class="about-row">
        <span class="about-row__label">{{ t("update.downloadUrl") }}</span>
        <div class="about-row__value update-url" :title="t('update.copyTitle')" @click="copyReleaseUrl">
          <a-input :model-value="releaseUrl" readonly size="small" />
        </div>
        <a-button size="small" @click="openReleaseUrl">{{ t("update.openUrl") }}</a-button>
      </div>
      <div class="hint-dim">{{ t("update.copyHint") }}</div>
      <!-- 两种版本的差别只影响「要不要更新」，程序不会自己下载或替换文件；
           页面上所有版本都能从发布页取到，由用户自行选择。 -->
      <div class="hint-dim">{{ t("update.kindHint") }}</div>
    </a-scrollbar>
  </a-modal>
</template>
