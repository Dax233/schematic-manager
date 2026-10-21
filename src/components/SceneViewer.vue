<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { t } from "../i18n";
import type { SceneData } from "../types";
import ViewGizmo from "./ViewGizmo.vue";
import {
  buildSceneGroup,
  defaultViewDir,
  fitCamera as fitCameraTo,
  frameBox,
  frameOrthoBox,
  orthoZoomLimits,
  presetDir,
  presetIsOrtho,
  syncClipPlanes,
  zoomLimits,
  type SavedViewPreset,
  type SceneGroup,
} from "../render/scene3d";

const props = withDefaults(
  defineProps<{
    scene: SceneData | null;
    /** 亮度系数，1.0 为原始 */
    brightness?: number;
  }>(),
  { brightness: 1.0 },
);

/** 方向标点了某个轴向时通知父级。
 *
 * 视角状态（activeView）由 store 持有，这里不直接改它——否则同一次操作会
 * 出现两条改状态的路径，界面上的高亮与实际视角迟早对不上。父级收到事件后
 * 调 store 的 setView，再由 store 回头调用本组件的 applyViewPreset。
 */
const emit = defineEmits<{
  (e: "pickView", key: string): void;
}>();

const host = ref<HTMLDivElement | null>(null);
const error = ref("");

// three 的对象不需要响应式代理（会被深度追踪，既慢又可能出错）
const renderer = shallowRef<THREE.WebGLRenderer | null>(null);
let built: SceneGroup | null = null;
let threeScene: THREE.Scene | null = null;
/** 透视相机：默认使用 */
let perspCamera: THREE.PerspectiveCamera | null = null;
/** 正交相机：切到「正射投影」时使用 */
let orthoCamera: THREE.OrthographicCamera | null = null;
/** 当前参与渲染与交互的那一个。
 *
 * 两个相机共享同一套位置与朝向，切换投影方式时只换对象、不重算视角，
 * 用户看到的是「同一个角度换了投影」，而不是画面忽然跳一下。
 */
let useOrtho = false;
let controls: OrbitControls | null = null;
// 按需渲染：只在场景或相机变化时重绘。持续跑 requestAnimationFrame
// 会让笔记本常态占用 GPU，而这是静态预览，不操作时画面不会变。
let raf = 0;
let needsRender = true;

/** 上一次结算时的已渲染帧数。每秒结算一次，差值就是这一秒的帧率。 */
let fpsFrames = 0;
let fpsMark = 0;

/** 实际渲染帧率。按需渲染下静止时是 0——这正是要看的信息：
 *  0 表示「当前没有在画」，而不是「卡死了」。
 *
 * 不计入 exportPng 里那一次离屏渲染：它不是主循环画的帧，
 * 算进去会让导出瞬间的读数虚高。 */
const fps = ref(0);

/** 相机朝向，供方向标绘制。只在朝向变化时更新——每帧写响应式 ref 会让
 *  Vue 持续重渲染，而方向标只在相机转动时才需要重画。 */
const orientation = ref<THREE.Quaternion | null>(null);

/** 把当前相机朝向同步给方向标 */
function syncOrientation() {
  const cam = activeCamera();
  orientation.value = cam ? cam.quaternion.clone() : null;
}

/** 累计一帧并每秒结算一次 */
function countFrame(now: number) {
  fpsFrames++;
  if (fpsMark === 0) {
    fpsMark = now;
    return;
  }
  const elapsed = now - fpsMark;
  if (elapsed < 1000) return;
  fps.value = Math.round((fpsFrames * 1000) / elapsed);
  fpsFrames = 0;
  fpsMark = now;
}

/** 每秒把读数拉回 0：停止拖动后不再渲染，但没人去改这个数，
 *  读数会一直停在最后一秒的值，看起来像还在跑。 */
let fpsIdle = 0;
function startFpsClock() {
  fpsIdle = window.setInterval(() => {
    // 这一秒内没有渲染过任何帧，说明画面是静止的
    if (fpsMark !== 0 && performance.now() - fpsMark >= 1000) {
      fps.value = 0;
      fpsFrames = 0;
      fpsMark = 0;
    }
  }, 1000);
}
// 容器尺寸变化用 ResizeObserver 而不是只监听 window：预览舞台会因
// 侧栏拖动、分隔条拖动、页签切换而改变大小，这些都不触发 window resize。
//
// 只重设画布尺寸、不重新取景：相机 FOV 是垂直方向的，而舞台的宽高比固定
// （398:344，由 composables/usePreviewResize 保证），因此画布变大后同一个
// 相机看到的是同一个角范围——模型占画面的**比例**不变而**像素**变多，
// 正好是「舞台变大就画得更大」。重新取景反而会把用户转好的视角掰回去。
let resizeObserver: ResizeObserver | null = null;

/** 请求在下一帧重绘 */
function requestRender() {
  needsRender = true;
}

/** 释放当前网格 */
function disposeScene() {
  built?.dispose();
  built = null;
}

/** 当前参与渲染与交互的相机。切换投影方式只换这一个。 */
function activeCamera(): THREE.PerspectiveCamera | THREE.OrthographicCamera | null {
  return useOrtho ? orthoCamera : perspCamera;
}

/**
 * 把相机拉到能完整看到整个投影的位置，并把**旋转轴心**放到模型中心。
 *
 * 轴心必须一起设：OrbitControls 绕 `target` 旋转，而 target 默认在原点。
 * 若只移动相机不改 target，结构偏离原点时就会「绕着世界原点转」——
 * 表现为拖动时模型绕着画面外的一个点甩动，越大的结构越明显。
 *
 * 两种相机的取景算法不同：透视按 FOV 解距离，正交按投影尺寸定视景体。
 */
function fitCamera() {
  const cam = activeCamera();
  if (!cam || !built) return;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) {
    // 空场景（理论上不会走到，几何建好后包围盒就不空）：交给透视那套给个固定位置
    if (cam instanceof THREE.OrthographicCamera) {
      cam.position.set(12, 12, 12);
      cam.lookAt(0, 0, 0);
      cam.updateProjectionMatrix();
    } else {
      fitCameraTo(cam, built.group);
    }
    renderSoon();
    return;
  }
  const center = box.getCenter(new THREE.Vector3());
  if (cam instanceof THREE.OrthographicCamera) {
    frameOrthoBox(cam, box, defaultViewDir(), orthoAspect());
  } else {
    fitCameraTo(cam, built.group);
  }
  controls?.target.copy(center);
  rememberFit();
  controls?.update();
  renderSoon();
}

/**
 * 记下本次自适应的基准，并据此定出滚轮缩放的上下限。
 *
 * 两种相机限的不是同一个量：透视限的是到目标的距离（OrbitControls 每次
 * update 都会 clampLength），正交限的是 camera.zoom（它没有距离概念）。
 * 裁剪面也只有透视需要跟着距离走。
 */
function rememberFit() {
  const cam = activeCamera();
  if (!cam || !controls) return;
  if (cam instanceof THREE.OrthographicCamera) {
    const { min, max } = orthoZoomLimits();
    controls.minZoom = min;
    controls.maxZoom = max;
    controls.enableZoom = true;
    return;
  }
  const dist = cam.position.distanceTo(controls.target);
  const { min, max } = zoomLimits(dist);
  controls.minDistance = min;
  controls.maxDistance = max;
  syncClipPlanes(cam, controls.target);
}

function applyPreset(name: string) {
  const cam = activeCamera();
  if (!cam || !built) return;
  const box = new THREE.Box3().setFromObject(built.group);
  if (box.isEmpty()) return;
  const dir = presetDir(name);
  const center = box.getCenter(new THREE.Vector3());
  if (cam instanceof THREE.OrthographicCamera) {
    const aspect = orthoAspect();
    frameOrthoBox(cam, box, dir, aspect);
  } else {
    frameBox(cam, box, dir);
  }
  // 换视角同样要把轴心留在模型中心，否则换完视角再拖动就会绕错点
  controls?.target.copy(center);
  rememberFit();
  controls?.update();
  renderSoon();
}

/** 正交相机的宽高比：它不存 aspect，要由视景体反推。
 *
 * 视景体在 onResize 里按容器宽高与上一个 aspect 重新算，因此这里读的是
 * 当前画布的真实比例。取不到时退回 1。
 */
function orthoAspect(): number {
  const el = host.value;
  if (!el || el.clientHeight === 0) return 1;
  return el.clientWidth / el.clientHeight;
}

/**
 * 立即绘制一帧。
 *
 * 供导出预览图等「必须马上拿到画面」的场景使用；
 * 常规刷新走 requestRender，由渲染循环按需重绘。
 */
function render() {
  const cam = activeCamera();
  if (renderer.value && threeScene && cam) {
    needsRender = false;
    renderer.value.render(threeScene, cam);
  }
}

/** 相机或场景变化后请求重绘 */
function renderSoon() {
  requestRender();
}

function onResize() {
  const el = host.value;
  const cam = activeCamera();
  if (!el || !renderer.value || !cam) return;
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (w === 0 || h === 0) return;
  renderer.value.setSize(w, h, false);
  if (cam instanceof THREE.OrthographicCamera) {
    // 正交相机的成像大小由视景体与 zoom 决定，宽高比变化时必须重算左右边界，
    // 否则窗口一变宽画面就会被横向拉伸
    resizeOrthoFrustum(cam, w / h);
  } else {
    cam.aspect = w / h;
    cam.updateProjectionMatrix();
  }
  renderSoon();
}

/** 按新的宽高比重设正交视景体，保持当前水平可视范围不变。
 *
 * 只改 top/bottom 并让 zoom 参与：直接改 left/right 会把用户的缩放一并抹掉。
 */
function resizeOrthoFrustum(cam: THREE.OrthographicCamera, aspect: number) {
  const halfH = cam.top / cam.zoom;
  const halfW = cam.right / cam.zoom;
  // 以「较宽的一侧为准」重算另一半，保证模型不被裁掉
  const newHalfH = halfW / aspect < halfH ? halfW / aspect : halfH;
  cam.left = -halfW;
  cam.right = halfW;
  cam.top = newHalfH;
  cam.bottom = -newHalfH;
  cam.updateProjectionMatrix();
}

/** 重建网格并刷新画面 */
function rebuild(data: SceneData | null) {
  disposeScene();
  if (!data) {
    // 清空后也要重绘一次，否则画布上会残留上一个投影的画面
    renderSoon();
    return;
  }
  built = buildSceneGroup(data);
  if (threeScene) threeScene.add(built.group);
  // 切换投影后必须显式请求重绘。
  //
  // 这是「切换投影后预览不更新，要点一下画面才变」的成因：按需渲染只在
  // needsRender 或阻尼未停时才画，而重建网格既不置 needsRender、也不改变
  // 相机（fitCamera 只调 controls.update()，没有输入时阻尼不产生位移），
  // 于是画布一直停在上一帧，直到用户点一下画面触发交互才刷新。
  renderSoon();
}

onMounted(() => {
  const el = host.value;
  if (!el) return;
  try {
    const r = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      // 导出预览图要在 render() 之后异步读取画布，
      // 不保留绘制缓冲会读到空白图
      preserveDrawingBuffer: true,
    });
    r.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    r.setClearColor(0x000000, 0);
    el.appendChild(r.domElement);
    renderer.value = r;

    const scene = new THREE.Scene();
    threeScene = scene;

    // 不加光源：材质是 MeshBasicMaterial，本就不参与光照计算，
    // 立体感完全来自逐顶点的明暗系数（写入 color 属性）。

    perspCamera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    // 正交相机先给一个占位视景体，真正的尺寸在 fitCamera 里按模型算
    orthoCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
    controls = makeControls(perspCamera, r.domElement);

    const loop = () => {
      raf = requestAnimationFrame(loop);
      let changed = controls?.update() ?? false;
      const cam = activeCamera();
      // 距离变了就要跟着改裁剪面，否则一直拉近会穿进近裁剪面、模型被切没。
      // 这项只对透视相机有意义：正交的裁剪面不随视角变化
      if (cam instanceof THREE.PerspectiveCamera && controls) {
        if (syncClipPlanes(cam, controls.target)) changed = true;
      }
      // 阻尼未停时必须每帧重绘，否则拖动动画会卡住
      if (needsRender || changed) {
        needsRender = false;
        if (renderer.value && cam) {
          renderer.value.render(scene, cam);
          countFrame(performance.now());
        }
      }
      // 方向标要跟着相机转：只在朝向真的变了时更新，
      // 否则每帧都写响应式 ref 会让 Vue 持续重渲染
      if (changed) syncOrientation();
    };
    loop();
    startFpsClock();

    window.addEventListener("resize", onResize);
    // 窗口 resize 事件只在窗口尺寸变化时触发，而预览舞台现在会随
    // **侧栏宽度、分隔条拖动、页签切换**等多种原因改变大小。
    // 只用 window 事件会让画布停留在旧尺寸（表现为图像被拉伸或裁掉）。
    if (typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => onResize());
      resizeObserver.observe(el);
    }
    onResize();
    if (props.scene) {
      rebuild(props.scene);
      fitCamera();
    }
  } catch (e) {
    // WebGL 不可用时给出明确提示，而不是留下一块空白
    error.value = t("scene.initFailed", { error: String(e) });
  }
});

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  window.clearInterval(fpsIdle);
  window.removeEventListener("resize", onResize);
  resizeObserver?.disconnect();
  resizeObserver = null;
  disposeScene();
  controls?.dispose();
  renderer.value?.dispose();
  renderer.value?.domElement.remove();
  renderer.value = null;
  threeScene = null;
  perspCamera = null;
  orthoCamera = null;
  controls = null;
});

watch(
  () => props.scene,
  (data) => {
    rebuild(data);
    if (data) fitCamera();
    // 亮度：新建材质时要套用当前值，否则切换投影后画面回到 100%
    // 而滑杆仍显示原值，两者不一致
    applyBrightness(props.brightness);
    renderSoon();
  },
);

/** 把亮度套到现有材质上 */
function applyBrightness(b: number) {
  built?.group.traverse((o) => {
    const m = (o as THREE.Mesh).material as THREE.MeshBasicMaterial | undefined;
    if (m && "color" in m && m.vertexColors) {
      // 顶点颜色已承载面明暗，亮度通过材质颜色整体缩放
      m.color.setScalar(b);
      m.needsUpdate = true;
    }
  });
}

watch(
  () => props.brightness,
  (b) => {
    applyBrightness(b);
    render();
  },
);

/**
 * 按指定像素宽高导出当前画面为透明背景 PNG。
 *
 * 临时把渲染器调到目标尺寸、按目标宽高比调整相机，渲染后立即恢复，
 * 因此用户不会看到界面尺寸被改动。
 *
 * 两种相机调宽高比的方式不同：透视改 aspect，正交要按比例重算视景体
 * （它没有 aspect 字段）。导出前后都要还原，否则界面里的构图会变。
 */
async function exportPng(w: number, h: number): Promise<Uint8Array> {
  const r = renderer.value;
  const cam = activeCamera();
  if (!r || !cam || !threeScene) {
    throw new Error(t("scene.notReady"));
  }
  const prev = new THREE.Vector2();
  r.getSize(prev);
  const prevRatio = r.getPixelRatio();
  const isOrtho = cam instanceof THREE.OrthographicCamera;
  const prevAspect = isOrtho ? 0 : cam.aspect;
  const prevFrustum = isOrtho
    ? { l: cam.left, r: cam.right, t: cam.top, b: cam.bottom, z: cam.zoom }
    : null;

  try {
    r.setPixelRatio(1);
    r.setSize(w, h, false);
    if (isOrtho) {
      resizeOrthoFrustum(cam, w / h);
    } else {
      cam.aspect = w / h;
      cam.updateProjectionMatrix();
    }
    r.render(threeScene, cam);

    const blob: Blob = await new Promise((resolve, reject) => {
      r.domElement.toBlob(
        (b) => (b ? resolve(b) : reject(new Error(t("scene.exportFailed")))),
        "image/png",
      );
    });
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    r.setPixelRatio(prevRatio);
    r.setSize(prev.x, prev.y, false);
    if (isOrtho && prevFrustum) {
      cam.left = prevFrustum.l;
      cam.right = prevFrustum.r;
      cam.top = prevFrustum.t;
      cam.bottom = prevFrustum.b;
      cam.zoom = prevFrustum.z;
      cam.updateProjectionMatrix();
    } else if (!isOrtho) {
      cam.aspect = prevAspect;
      cam.updateProjectionMatrix();
    }
    render();
  }
}

/** 创建并配置一台 OrbitControls。切换相机时必须重建：
 *  它在构造时绑死相机实例，没有换相机的接口。 */
function makeControls(cam: THREE.Camera, dom: HTMLElement): OrbitControls {
  const c = new OrbitControls(cam, dom);
  c.enableDamping = true;
  c.dampingFactor = 0.15;
  c.screenSpacePanning = true;
  c.enableZoom = true;
  c.zoomSpeed = 0.9;
  return c;
}

/** 切换正射投影与透视投影。
 *
 * 两个相机共享位置与朝向：切换时把当前相机的位姿拷给另一个，让用户看到的是
 * 「同一角度换了投影」，画面不会跳位置。OrbitControls 绑在具体相机实例上，
 * 因此必须重建它——顺序上先重建、再按新相机取景，否则会拿着旧 controls 的
 * target 去算朝向。
 */
function setOrtho(ortho: boolean) {
  if (useOrtho === ortho) return;
  const from = activeCamera();
  const prevTarget = controls?.target.clone() ?? new THREE.Vector3();
  useOrtho = ortho;
  const to = activeCamera();
  if (!to) return;

  if (from) {
    to.position.copy(from.position);
    to.quaternion.copy(from.quaternion);
    to.up.copy(from.up);
  }

  const dom = renderer.value?.domElement;
  if (dom) {
    controls?.dispose();
    controls = makeControls(to, dom);
    controls.target.copy(prevTarget);
  }

  if (to instanceof THREE.OrthographicCamera && built) {
    const box = new THREE.Box3().setFromObject(built.group);
    if (!box.isEmpty()) {
      // 沿用当前朝向（相机 → 目标），只把视景体按新宽高比重算
      const dir = new THREE.Vector3().subVectors(prevTarget, to.position);
      if (dir.lengthSq() > 0) {
        frameOrthoBox(to, box, dir.normalize(), orthoAspect());
      }
    }
  }

  rememberFit();
  controls?.update();
  renderSoon();
}

/** 当前是否为正射投影 */
function isOrtho(): boolean {
  return useOrtho;
}

/** 当前相机的朝向（四元数副本），供方向标把三个轴投到屏幕 */
function cameraQuaternion(): THREE.Quaternion | null {
  const cam = activeCamera();
  return cam ? cam.quaternion.clone() : null;
}

/** 当前相机的位姿，供「保存为预设」记录。
 *
 * 直接给位置与目标点而不是只给角度：位置决定了透视下的观感（距离远近），
 * 只存角度会让存下来的预设一用就变成「同一个方向、不同的远近」。
 */
function cameraPose(): {
  position: [number, number, number];
  target: [number, number, number];
} | null {
  const cam = activeCamera();
  if (!cam || !controls) return null;
  return {
    position: [cam.position.x, cam.position.y, cam.position.z],
    target: [controls.target.x, controls.target.y, controls.target.z],
  };
}

/** 应用一条自定义预设：按存下来的位置与目标点还原视角。
 *
 * 与内置预设不同，这里不走 fitDistance 重新取景——用户存的是「他看到的
 * 那个画面」，按模型重新算距离会让缩放过的视角被掰回自适应位置。
 */
function applySavedView(preset: SavedViewPreset) {
  setOrtho(preset.ortho);
  const cam = activeCamera();
  if (!cam || !controls) return;
  cam.position.set(...preset.position);
  controls.target.set(...preset.target);
  cam.lookAt(controls.target);
  if (cam instanceof THREE.OrthographicCamera && built) {
    // 正交相机的位置不影响成像大小，但视景体要按当前朝向重算，
    // 否则横向被裁——它不能沿用别的朝向算出来的左右边界
    const box = new THREE.Box3().setFromObject(built.group);
    if (!box.isEmpty()) {
      const dir = new THREE.Vector3().subVectors(controls.target, cam.position);
      if (dir.lengthSq() > 0) {
        frameOrthoBox(cam, box, dir.normalize(), orthoAspect());
        // 位置被 frameOrthoBox 改写了，按存档再摆回来
        cam.position.set(...preset.position);
        cam.lookAt(controls.target);
      }
    }
  }
  rememberFit();
  controls.update();
  syncOrientation();
  renderSoon();
}

/** 按预设键切换视角：投影方式由预设自己决定（真等距是正射，其余是透视）。
 *
 * 方向标与右键菜单都走这里，确保「同一个预设」在两条入口下行为一致。
 */
function applyViewPreset(name: string) {
  setOrtho(presetIsOrtho(name));
  applyPreset(name);
  syncOrientation();
}

defineExpose({
  applyPreset,
  applyViewPreset,
  exportPng,
  fitCamera,
  setOrtho,
  isOrtho,
  cameraQuaternion,
  cameraPose,
  applySavedView,
});
</script>

<template>
  <div class="scene-viewer">
    <div ref="host" class="scene-viewer__canvas" />
    <!-- 帧率：按需渲染下静止时是 0，这一读数用来判断画面是否还在重绘
         （拖动视角时会跳动，停手后归零）。只在有场景且 WebGL 正常时显示。 -->
    <div
      v-if="scene && !error"
      class="scene-viewer__fps"
      :class="{ 'is-idle': fps === 0 }"
      :title="fps === 0 ? t('scene.fpsIdle') : t('scene.fps', { value: fps })"
    >
      {{ fps === 0 ? t("scene.fpsIdle") : t("scene.fps", { value: fps }) }}
    </div>
    <div v-if="error" class="scene-viewer__error">{{ error }}</div>
    <div v-else-if="!scene" class="scene-viewer__empty">{{ t("scene.empty") }}</div>
    <!-- XYZ 方向标：显示当前朝向的三个坐标轴，点端点小球切到对应轴视角。
         没有场景时不显示——那时没有任何东西可以「看」。 -->
    <ViewGizmo
      v-if="scene && !error"
      class="scene-viewer__gizmo"
      :orientation="orientation"
      @pick="(key) => emit('pickView', key)"
      @reset="fitCamera()"
    />
  </div>
</template>

<style scoped lang="scss">
.scene-viewer {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 200px;
  // 裁掉任何越界内容，作为「渲染图形不得超出范围」的最后一道防线
  overflow: hidden;

  &__canvas {
    // 画布容器：占满父容器（预览舞台），实际宽高由 .preview__stage 决定
    width: 100%;
    height: 100%;
    overflow: hidden;

    // three.js 的 setSize(w, h, false) 只设置 canvas 的 width/height **属性**，
    // 不改 CSS 尺寸。若不在这里约束 canvas 元素本身，它会按属性尺寸显示：
    // 实测容器 862px 而画布 987px，横向溢出 125px，画面被撑出面板之外。
    // 用 100% 让显示尺寸恒等于容器，位图分辨率另由属性决定（设备像素比放大）。
    :deep(canvas) {
      display: block;
      width: 100%;
      height: 100%;
    }
  }

  &__error,
  &__empty {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-3);
    font-size: 13px;
    pointer-events: none;
  }

  // 帧率读数：画面右上角，半透明底避免压住模型细节。
  // 静止时（0 帧）压暗，一眼能区分「在重绘」与「停着」。
  &__fps {
    position: absolute;
    top: 6px;
    right: 6px;
    padding: 1px 6px;
    border-radius: var(--border-radius-small);
    background: var(--color-bg-popup);
    color: var(--color-text-2);
    font-family: Consolas, "Courier New", monospace;
    font-size: 11px;
    line-height: 16px;
    opacity: 0.8;
    pointer-events: none;

    &.is-idle {
      opacity: 0.45;
    }
  }

  &__error {
    color: rgb(var(--danger-6));
  }

  // 方向标：画面左下角。放右下角会与 FPS 读数打架，
  // 且左下角在多数投影图里是空白区（模型居中）。
  &__gizmo {
    position: absolute;
    left: 6px;
    bottom: 6px;
  }
}
</style>
