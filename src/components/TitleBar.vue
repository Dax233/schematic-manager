/**
 * 自定义标题栏：应用标识 + 窗口控制按钮（最小化/最大化/关闭）。
 *
 * 窗口外壳行为，与业务状态无关，因此直接在这里调 Tauri 窗口 API，
 * 不进 store。拖动区由 data-tauri-drag-region 标记，交给系统处理。
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
    <div class="titlebar__logo">图</div>
    <div class="titlebar__title">投影文件管理器</div>
    <div class="titlebar__spacer" data-tauri-drag-region />
    <div class="titlebar__btn" @click="minimize">&#x2500;</div>
    <div class="titlebar__btn" @click="toggleMaximize">&#x25A1;</div>
    <div class="titlebar__btn is-close" @click="close">&#x2715;</div>
  </div>
</template>
