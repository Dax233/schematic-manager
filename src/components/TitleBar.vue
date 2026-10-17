/**
 * 自定义标题栏：应用标识 + 窗口控制按钮（最小化/最大化/关闭）。
 *
 * 窗口外壳行为，与业务状态无关，因此直接在这里调 Tauri 窗口 API，
 * 不进 store。拖动区由 data-tauri-drag-region 标记，交给系统处理。
 *
 * 左上角的标识与程序图标同源：深色圆角方块 + 青色等轴测立方体 + 四角选中括号。
 * 用内联 SVG 而不是图片，是为了任意缩放都清晰、也不额外引入资源文件；坐标按
 * 18px 直接算好，线宽与图标生成脚本在 18px 档的取值一致（细线在小尺寸下会糊）。
 */
<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";

const appWindow = getCurrentWindow();
const minimize = () => appWindow.minimize();
const toggleMaximize = () => appWindow.toggleMaximize();
const close = () => appWindow.close();
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="titlebar__logo" aria-hidden="true">
      <svg viewBox="0 0 18 18" width="18" height="18">
        <!-- 立方体：顶面菱形、左右两侧面，共用一个虚接的中心点 -->
        <g fill="none" stroke="#7ec8ff" stroke-width="1.35" stroke-linejoin="round">
          <polygon points="3.87,6.75 9,4.185 14.13,6.75 9,9.315" />
          <polygon points="3.87,6.75 9,9.315 9,14.445 3.87,11.88" />
          <polygon points="9,9.315 14.13,6.75 14.13,11.88 9,14.445" />
        </g>
        <!-- 四角选中括号 -->
        <g fill="none" stroke="#ffffff" stroke-width="1.6">
          <path d="M1.89 4.68 V1.89 H4.68" />
          <path d="M13.32 1.89 H16.11 V4.68" />
          <path d="M1.89 13.32 V16.11 H4.68" />
          <path d="M13.32 16.11 H16.11 V13.32" />
        </g>
      </svg>
    </div>
    <div class="titlebar__title">投影文件管理器</div>
    <div class="titlebar__spacer" data-tauri-drag-region />
    <div class="titlebar__btn" @click="minimize">&#x2500;</div>
    <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
    <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
  </div>
</template>
