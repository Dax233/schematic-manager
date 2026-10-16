/**
 * 更新窗口：显示当前版本与作者，并可检测 GitHub 上有没有新版本。
 *
 * 版本号取自后端 `app_version`（编译进二进制的包版本），不是界面里的常量：
 * CI 在构建前才把版本号写入 Cargo.toml，界面若自己写一份，两者迟早对不上。
 *
 * 检测结果分四种，分别用不同措辞，避免把「没查到」说成「已是最新」：
 * 有更新 / 已是最新 / 仓库里没有可解析的发布 / 检测失败（附原因）。
 *
 * 发布页地址用只读输入框展示而不是做成可点击链接：本项目没有引入打开外部
 * 链接的插件，做不成真正的跳转；只读输入框可以直接选中复制，比一个点了
 * 没反应的链接诚实。
 */
<script setup lang="ts">
import { useAppStore } from "../store";

const {
  updateOpen,
  currentVersion,
  updateInfo,
  updateChecking,
  updateError,
  runUpdateCheck,
} = useAppStore();

/** 作者信息：按用户要求展示 */
const AUTHOR = "bilibili黑山大叔";
/** 发布页总是可以在浏览器里打开的地址，检测前也要能看到 */
const RELEASES_PAGE = "https://github.com/heiheids/schematic-manager/releases";
</script>

<template>
  <a-modal
    v-model:visible="updateOpen"
    title="更新"
    :width="520"
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

      <div class="about-row">
        <span class="about-row__label">发布页</span>
        <a-input
          class="about-row__value"
          :model-value="updateInfo.releaseUrl || RELEASES_PAGE"
          readonly
          size="small"
        />
      </div>
      <div class="hint-dim">选中上面的地址复制到浏览器即可下载。</div>
    </template>

    <div v-else class="hint-dim">点「检测更新」查看是否有新版本。</div>
  </a-modal>
</template>
