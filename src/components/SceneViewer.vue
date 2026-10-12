<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { SceneData } from "../types";

const props = withDefaults(
  defineProps<{
    scene: SceneData | null;
    /** 亮度系数，1.0 为原始 */
    brightness?: number;
  }>(),
  { brightness: 1.0 },
);

const host = ref<HTMLDivElement | null>(null);
const error = ref("");

// three 的对象不需要响应式代理（会被深度追踪，既慢又可能出错）
const renderer = shallowRef<THREE.WebGLRenderer | null>(null);
const root = shallowRef<THREE.Group | null>(null);
let threeScene: THREE.Scene | null = null;
let camera: THREE.PerspectiveCamera | null = null;
let controls: OrbitControls | null = null;
// 按需渲染：只在场景或相机变化时重绘。持续跑 requestAnimationFrame
// 会让笔记本常态占用 GPU，而这是静态预览，不操作时画面不会变。
let raf = 0;
let needsRender = true;

/** 请求在下一帧重绘 */
function requestRender() {
  needsRender = true;
}
const disposables: Array<{ dispose: () => void }> = [];

const PRESETS: Record<string, [number, number]> = {
  front: [0, 12],
  left45: [-45, 30],
  right45: [45, 30],
  back: [180, 12],
};

function disposeScene() {
  for (const d of disposables) d.dispose();
  disposables.length = 0;
  if (root.value) {
    root.value.clear();
  }
}

/** 由场景数据构建 three 的网格 */
function buildMeshes(data: SceneData) {
  if (!root.value) return;
  disposeScene();

  for (const batch of data.batches) {
    const geo = new THREE.BufferGeometry();
    // 顶点位置已在世界坐标（单位为方块），x/z 需要居中
    const pos = new Float32Array(batch.positions);
    for (let i = 0; i < pos.length; i += 3) {
      pos[i] -= data.origin[0];
      pos[i + 1] -= data.origin[1];
      pos[i + 2] -= data.origin[2];
    }
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
    // UV 在模型空间为 0..16，除以 16 归一化
    const uvRaw = new Float32Array(batch.uvs);
    const uv = new Float32Array(uvRaw.length);
    for (let i = 0; i < uvRaw.length; i++) uv[i] = uvRaw[i] / 16;
    geo.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
    // 明暗系数作为第二套 UV 之外的逐顶点属性，写进顶点颜色
    const shade = new Float32Array(batch.shades);
    const color = new Float32Array(shade.length * 3);
    for (let i = 0; i < shade.length; i++) {
      color[i * 3] = shade[i];
      color[i * 3 + 1] = shade[i];
      color[i * 3 + 2] = shade[i];
    }
    geo.setAttribute("color", new THREE.BufferAttribute(color, 3));
    geo.setIndex(new THREE.BufferAttribute(new Uint32Array(batch.indices), 1));
    geo.computeBoundingBox();

    const tex = new THREE.DataTexture(
      new Uint8Array(batch.textureRgba),
      batch.textureWidth,
      batch.textureHeight,
      THREE.RGBAFormat,
    );
    // Minecraft 贴图按方块单位采样，必须最近邻，否则边缘会糊
    tex.magFilter = THREE.NearestFilter;
    tex.minFilter = THREE.NearestFilter;
    tex.generateMipmaps = false;
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.needsUpdate = true;

    const mat = new THREE.MeshBasicMaterial({
      map: tex,
      vertexColors: true,
      side: THREE.FrontSide,
      transparent: true,
      alphaTest: 0.5,
    });
    // 立即套用当前亮度：切换投影会重建材质，若不在创建时设置，
    // 画面会回到 100% 而滑杆仍显示原值，两者不一致
    mat.color.setScalar(props.brightness);

    const mesh = new THREE.Mesh(geo, mat);
    root.value.add(mesh);
    disposables.push(geo, tex, mat);
  }
}

/** 把相机拉到能完整看到整个投影的位置 */
function fitCamera(data: SceneData) {
  if (!camera || !controls || !root.value) return;
  const box = new THREE.Box3().setFromObject(root.value);
  const size = box.getSize(new THREE.Vector3());
  const center = box.getCenter(new THREE.Vector3());
  const radius = Math.max(size.length() * 0.5, 1);
  const dist = radius / Math.sin((camera.fov * Math.PI) / 360);
  controls.target.copy(center);
  camera.position.copy(center).add(new THREE.Vector3(dist, dist, dist));
  camera.near = Math.max(dist / 100, 0.05);
  camera.far = dist * 10;
  camera.updateProjectionMatrix();
  controls.update();
}

function applyPreset(name: string) {
  const angles = PRESETS[name];
  if (!angles || !camera || !controls || !root.value) return;
  const box = new THREE.Box3().setFromObject(root.value);
  const center = box.getCenter(new THREE.Vector3());
  const radius = Math.max(box.getSize(new THREE.Vector3()).length() * 0.5, 1);
  const dist = radius / Math.sin((camera.fov * Math.PI) / 360);
  const [yawDeg, pitchDeg] = angles;
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  // 与旧版一致的视角定义：yaw 绕 Y，pitch 为俯角
  const dir = new THREE.Vector3(
    Math.cos(pitch) * Math.sin(yaw),
    -Math.sin(pitch),
    Math.cos(pitch) * Math.cos(yaw),
  );
  controls.target.copy(center);
  camera.position.copy(center).addScaledVector(dir, -dist);
  controls.update();
  renderSoon();
}

/**
 * 立即绘制一帧。
 *
 * 供导出预览图等「必须马上拿到画面」的场景使用；
 * 常规刷新走 requestRender，由渲染循环按需重绘。
 */
function render() {
  if (renderer.value && threeScene && camera) {
    needsRender = false;
    renderer.value.render(threeScene, camera);
  }
}

/** 相机或场景变化后请求重绘 */
function renderSoon() {
  requestRender();
}

function onResize() {
  const el = host.value;
  if (!el || !renderer.value || !camera) return;
  const w = el.clientWidth;
  const h = el.clientHeight;
  if (w === 0 || h === 0) return;
  renderer.value.setSize(w, h, false);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
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

    const group = new THREE.Group();
    scene.add(group);
    root.value = group;

    camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    controls = new OrbitControls(camera, r.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.15;
    controls.screenSpacePanning = true;

    const loop = () => {
      raf = requestAnimationFrame(loop);
      const damping = controls?.update() ?? false;
      // 阻尼未停时必须每帧重绘，否则拖动动画会卡住
      if (needsRender || damping) {
        needsRender = false;
        if (renderer.value && camera) {
          renderer.value.render(scene, camera);
        }
      }
    };
    loop();

    window.addEventListener("resize", onResize);
    onResize();
    if (props.scene) {
      buildMeshes(props.scene);
      fitCamera(props.scene);
    }
  } catch (e) {
    // WebGL 不可用时给出明确提示，而不是留下一块空白
    error.value = `初始化 3D 视图失败：${String(e)}`;
  }
});

onBeforeUnmount(() => {
  cancelAnimationFrame(raf);
  window.removeEventListener("resize", onResize);
  disposeScene();
  controls?.dispose();
  renderer.value?.dispose();
  renderer.value?.domElement.remove();
  renderer.value = null;
  threeScene = null;
  camera = null;
  controls = null;
});

watch(
  () => props.scene,
  (data) => {
    if (!data) {
      disposeScene();
      return;
    }
    buildMeshes(data);
    fitCamera(data);
  },
);

watch(
  () => props.brightness,
  (b) => {
    if (!root.value) return;
    root.value.traverse((o) => {
      const m = (o as THREE.Mesh).material as THREE.MeshBasicMaterial | undefined;
      if (m && "color" in m && m.vertexColors) {
        // 顶点颜色已承载面明暗，亮度通过材质颜色整体缩放
        m.color.setScalar(b);
        m.needsUpdate = true;
      }
    });
    render();
  },
);

/**
 * 按指定边长离屏渲染并返回 PNG 字节。
 *
 * 导出与屏幕上看到的画面保持一致：临时把渲染器调到目标尺寸、
 * 相机宽高比设为 1、清屏色设为全透明，渲染后立即恢复原状，
 * 因此用户不会看到界面尺寸被改动。
 */
async function exportPng(size: number): Promise<Uint8Array> {
  const r = renderer.value;
  if (!r || !camera || !threeScene) {
    throw new Error("3D 视图尚未就绪，无法导出");
  }
  const prev = new THREE.Vector2();
  r.getSize(prev);
  const prevRatio = r.getPixelRatio();
  const prevAspect = camera.aspect;

  try {
    r.setPixelRatio(1);
    r.setSize(size, size, false);
    camera.aspect = 1;
    camera.updateProjectionMatrix();
    r.setClearAlpha(0); // 透明背景
    r.render(threeScene, camera);

    const blob: Blob = await new Promise((resolve, reject) => {
      r.domElement.toBlob((b) => (b ? resolve(b) : reject(new Error("导出图像失败"))), "image/png");
    });
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    r.setPixelRatio(prevRatio);
    r.setSize(prev.x, prev.y, false);
    camera.aspect = prevAspect;
    camera.updateProjectionMatrix();
    r.setClearAlpha(0);
    render();
  }
}

defineExpose({
  applyPreset,
  exportPng,
  fitCamera: () => props.scene && fitCamera(props.scene),
});
</script>

<template>
  <div class="scene-viewer">
    <div ref="host" class="scene-viewer__canvas" />
    <div v-if="error" class="scene-viewer__error">{{ error }}</div>
    <div v-else-if="!scene" class="scene-viewer__empty">尚无预览</div>
  </div>
</template>

<style scoped lang="scss">
.scene-viewer {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 240px;

  &__canvas {
    width: 100%;
    height: 100%;
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

  &__error {
    color: rgb(var(--danger-6));
  }
}
</style>
