/**
 * XYZ 三向方向标：显示当前视角下三个坐标轴各指向哪个方向，点端点小球可换到
 * 对应轴的正视角。
 *
 * 它是「当前朝向的读数」而不是独立的 3D 场景：用 SVG 把六个轴向按相机朝向
 * 投到屏幕上即可，不必再开一个 WebGL 上下文——那要多一份渲染循环，且与主画面
 * 的朝向同步容易不同步。
 *
 * 投影方式：把世界坐标里的轴向向量乘上「相机旋转的逆」，得到该轴在相机
 * 坐标系里的方向；x/y 就是它在屏幕上的横向与纵向分量，z 决定它朝向观察者
 * 还是远离（用于压暗背面）。
 */
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import * as THREE from "three";
import { gizmoDots } from "../render/view_gizmo";

const { t } = useI18n();

const props = defineProps<{
  /** 相机当前的朝向；null 表示还没有相机，方向标不显示轴向 */
  orientation: THREE.Quaternion | null;
}>();

const emit = defineEmits<{
  /** 点了某个轴向，参数是视角预设的键 */
  (e: "pick", key: string, axis: string): void;
  /** 点了中心方块，回到默认等距视角 */
  (e: "reset"): void;
}>();

/** 方向标半径（SVG 内的像素） */
const R = 34;
/** 六个端点小球的位置与深度，按远近排好序（远的先画） */
const dots = computed(() => gizmoDots(props.orientation, R));

/** 三个轴的颜色。X/Y/Z 用红绿蓝是工程制图与 3D 软件的通行约定，
 *  不宜改用主题色——那会让「哪根是哪个轴」失去通用性。
 *  深色主题下提亮一档，避免在深底上发闷。 */
function axisColor(axis: string): string {
  if (axis.endsWith("x")) return "var(--gizmo-x)";
  if (axis.endsWith("y")) return "var(--gizmo-y)";
  return "var(--gizmo-z)";
}

function onPick(key: string, axis: string) {
  emit("pick", key, axis);
}

/** 无障碍标签：轴向用「东（X+）」这类说法，与右键菜单的预设名一致 */
function ariaOf(axis: string): string {
  const map: Record<string, string> = {
    "+x": "view.east",
    "-x": "view.west",
    "+y": "view.top",
    "-y": "view.bottom",
    "+z": "view.south",
    "-z": "view.north",
  };
  return t(map[axis] ?? "view.top");
}
</script>

<template>
  <svg
    class="view-gizmo"
    :viewBox="`${-R - 12} ${-R - 12} ${(R + 12) * 2} ${(R + 12) * 2}`"
    width="104"
    height="104"
    role="group"
    :aria-label="t('view.gizmoLabel')"
  >
    <!-- 三根轴线：从中心连到对应端点。先画线再画球，球才盖在线上 -->
    <g>
      <line
        v-for="d in dots"
        :key="`line-${d.axis}`"
        :x1="0"
        :y1="0"
        :x2="d.x"
        :y2="d.y"
        :stroke="axisColor(d.axis)"
        :stroke-width="d.depth < 0 ? 1.2 : 2"
        :opacity="d.depth < 0 ? 0.45 : 0.95"
        :stroke-linecap="'round'"
      />
    </g>
    <!-- 端点小球：可点击，点一下切到该轴正视角 -->
    <g>
      <circle
        v-for="d in dots"
        :key="d.axis"
        :cx="d.x"
        :cy="d.y"
        r="9"
        class="view-gizmo__dot"
        :fill="axisColor(d.axis)"
        :opacity="d.depth < 0 ? 0.55 : 1"
        @click="onPick(d.key, d.axis)"
      >
        <title>{{ ariaOf(d.axis) }}</title>
      </circle>
      <!-- 轴向文字：同样随相机转动，随小球位置排布 -->
      <text
        v-for="d in dots"
        :key="`t-${d.axis}`"
        :x="d.x"
        :y="d.y + 3.2"
        class="view-gizmo__label"
        text-anchor="middle"
        :opacity="d.depth < 0 ? 0.6 : 1"
      >
        {{ d.axis }}
      </text>
    </g>
    <!-- 中心复位：回到默认等距视角 -->
    <g>
      <rect
        x="-7"
        y="-7"
        width="14"
        height="14"
        rx="2"
        class="view-gizmo__center"
        @click="emit('reset')"
      >
        <title>{{ t("view.reset") }}</title>
      </rect>
    </g>
  </svg>
</template>
