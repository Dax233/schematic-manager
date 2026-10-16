/**
 * 更新窗口：显示当前版本、作者与作者主页，并可检测 GitHub 上有没有新版本。
 *
 * 版本号取自后端 `app_version`（编译进二进制的包版本），不是界面里的常量：
 * CI 在构建前才把版本号写入 Cargo.toml，界面若自己写一份，两者迟早对不上。
 *
 * 检测结果分四种，分别用不同措辞，避免把「没查到」说成「已是最新」：
 * 有更新 / 已是最新 / 仓库里没有可解析的发布 / 检测失败（附原因）。
 *
 * 下载地址与作者主页都交给系统浏览器打开（后端 open_url）。下载地址框点击
 * 即复制——地址通常用来贴给别人，选中再按 Ctrl+C 是多一步。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

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
</script>

<template>
  <a-modal
    v-model:visible="updateOpen"
    title="更新"
    :width="600"
    :footer="false"
    unmount-on-close
  >
    <div class="about-row">
      <span class="about-row__label">当前版本</span>
      <span class="about-row__value">{{ currentVersion || "未知" }}</span>
    </div>
    <div class="about-row">
      <span class="about-row__label">作者</span>
      <span class="about-row__value">{{ AUTHOR }}</span>
    </div>
    <div class="about-row">
      <span class="about-row__label">作者主页</span>
      <span class="about-row__value">
        <a-link class="about-link" @click="openAuthorPage">{{ authorPage }}</a-link>
      </span>
    </div>
    <div class="about-row">
      <span class="about-row__label">检测目标</span>
      <span class="about-row__value">{{ updateInfo?.repo || "heiheids/schematic-manager" }}</span>
    </div>

    <a-space style="margin: 16px 0 8px">
      <a-button type="primary" size="small" :loading="updateChecking" @click="runUpdateCheck">
        检测更新
      </a-button>
      <span class="hint-dim">需要联网，读取 GitHub 上的发布列表</span>
    </a-space>

    <!-- 检测失败：把原因如实显示出来。这里最容易出错的表述是把网络失败
         说成「已是最新」，用户会以为确实没有新版本。 -->
    <a-alert v-if="updateError" type="error" style="margin-bottom: 8px">
      {{ updateError }}
    </a-alert>

    <template v-else-if="updateInfo">
      <a-alert v-if="updateInfo.hasUpdate" type="success" style="margin-bottom: 8px">
        发现新版本 {{ updateInfo.latest }}
      </a-alert>
      <a-alert v-else-if="updateInfo.latest" type="info" style="margin-bottom: 8px">
        已是最新版本（最新发布 {{ updateInfo.latest }}）
      </a-alert>
      <!-- 查到了接口、但里面没有可解析的发布，这与「最新」是两回事 -->
      <a-alert v-else type="warning" style="margin-bottom: 8px">
        没有查到任何发布
      </a-alert>

      <div v-if="updateInfo.publishedAt" class="hint-dim" style="margin-bottom: 8px">
        发布时间：{{ updateInfo.publishedAt }}
      </div>
    </template>

    <div v-else-if="!updateError" class="hint-dim" style="margin-bottom: 8px">
      点「检测更新」查看是否有新版本。
    </div>

    <!-- 下载地址：未检测时也给出发布列表页，随时可打开或复制 -->
    <div class="about-row">
      <span class="about-row__label">下载地址</span>
      <div
        class="about-row__value update-url"
        title="点击复制地址"
        @click="copyReleaseUrl"
      >
        <a-input :model-value="releaseUrl" readonly size="small" />
      </div>
      <a-button size="small" @click="openReleaseUrl">打开下载地址</a-button>
    </div>
    <div class="hint-dim">点击地址即复制；「打开下载地址」用浏览器打开该页面。</div>
  </a-modal>
</template>
