//! 由场景数据构建 three.js 网格，以及离屏渲染成 PNG。
//!
//! 抽成独立模块的原因：批量导出需要渲染**当前没有显示**的投影。
//! 若复用界面上的那个 viewer，就必须先把界面切到每个文件、
//! 等它渲染完再导出，过程既慢又会把用户正在看的画面反复冲掉。
//!
//! 因此这里提供一个独立的离屏渲染器；界面的 SceneViewer 复用其中的
//! 网格构建与取景逻辑，避免两处各写一份而逐渐不一致。

import * as THREE from "three";
import { t } from "../i18n";
import type { SceneData } from "../types";

/** 视角预设，与界面上的四个按钮一致 */
export const VIEW_PRESETS: Record<string, [number, number]> = {
  front: [0, 12],
  left45: [-45, 30],
  right45: [45, 30],
  back: [180, 12],
};

/** 一组已构建的网格及其需要释放的资源 */
export interface SceneGroup {
  group: THREE.Group;
  dispose: () => void;
}

/**
 * 由场景数据构建网格。
 *
 * 每个批次一张贴图、一个材质。顶点位置减去 origin 以居中；
 * UV 在模型空间是 0..16，除以 16 归一化。
 */
export function buildSceneGroup(data: SceneData): SceneGroup {
  const group = new THREE.Group();
  const disposables: Array<{ dispose: () => void }> = [];

  for (const batch of data.batches) {
    const geo = new THREE.BufferGeometry();
    const pos = new Float32Array(batch.positions);
    for (let i = 0; i < pos.length; i += 3) {
      pos[i] -= data.origin[0];
      pos[i + 1] -= data.origin[1];
      pos[i + 2] -= data.origin[2];
    }
    geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));

    const uvRaw = new Float32Array(batch.uvs);
    const uv = new Float32Array(uvRaw.length);
    for (let i = 0; i < uvRaw.length; i++) uv[i] = uvRaw[i] / 16;
    geo.setAttribute("uv", new THREE.BufferAttribute(uv, 2));

    // 顶点颜色直接来自后端：它已经把「面明暗」与「方块染色」乘好。
    // 材质不参与光照计算，立体感与染色全部由这里承载。
    geo.setAttribute(
      "color",
      new THREE.BufferAttribute(new Float32Array(batch.colors), 3),
    );
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

    // 按贴图的透明类型决定绘制方式。三者必须分开：
    //
    // - opaque：全不透明，正常写深度，正常互相遮挡。
    // - cutout：只有全透明与全不透明像素（树叶、草、藤蔓）。
    //   用 alphaTest 恰好合适——它把全透明像素丢掉，画出来的部分照常写深度。
    //   若改用混合，树叶之间会互相透视，看着像半透明的塑料片。
    //   阈值取 0.5：贴图只有 0 与 255，取任何 0..1 之间的值结果都一样。
    // - translucent：含真正的半透明像素（染色玻璃、水）。**不能用 alphaTest**：
    //   实测白色/黄色染色玻璃的贴图各有 191 个像素的 alpha 落在 1..127，
    //   它们会被整片丢弃，玻璃上出现窟窿。必须混合，并且不写深度，
    //   否则半透明的面会把后面的几何挡掉。
    const isCutout = batch.alphaMode === "cutout";
    const isTranslucent = batch.alphaMode === "translucent";
    const mat = new THREE.MeshBasicMaterial({
      map: tex,
      vertexColors: true,
      // 后端给出的三角形绕序已使其法线朝外，因此正面渲染即可。
      // 用 DoubleSide 会同时画出方块内部的面，透视效果反而不对。
      side: THREE.FrontSide,
      transparent: isTranslucent,
      alphaTest: isCutout ? 0.5 : 0,
      depthWrite: !isTranslucent,
    });

    const mesh = new THREE.Mesh(geo, mat);
    group.add(mesh);
    disposables.push(geo, tex, mat);
  }

  return {
    group,
    dispose: () => {
      for (const d of disposables) d.dispose();
      disposables.length = 0;
      group.clear();
    },
  };
}

/**
 * 求「把整个网格装进视口」所需的相机距离。
 *
 * 不能用外接球半径：外接球按对角线算直径，对细长结构等于要求把
 * 「对角线那么长的球」塞进视口高度，而结构在屏幕上的实际投影高度可能只有
 * 它的几分之一。实测一个 53×9×9 的机器只占画面 44.6%，四周一大片空白。
 *
 * 这里按**投影后的包围盒**拟合：把包围盒八个角点投到相机的横/纵轴上，
 * 分别求两个方向需要的最小距离，取较大者。对某个角点（相对中心偏移 x,y,z，
 * z 为正表示更远），它到相机的距离是 d+z，要落进视锥就要求
 * `|y| <= (d+z)·tan(fovY/2)` 与 `|x| <= (d+z)·tan(fovX/2)`，解出 d 的下界。
 *
 * `margin` 是留白系数：除以它相当于把可用视锥收窄一点，模型不至于顶到边。
 */
export function fitDistance(
  box: THREE.Box3,
  dir: THREE.Vector3,
  fovDeg: number,
  aspect: number,
  margin = 1.08,
): number {
  const center = box.getCenter(new THREE.Vector3());
  const forward = dir.clone().normalize();
  // 视线与世界上方平行时叉乘会退化，换一个不平行的参考方向
  const upHint =
    Math.abs(forward.y) > 0.99 ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(0, 1, 0);
  const right = new THREE.Vector3().crossVectors(forward, upHint).normalize();
  const up = new THREE.Vector3().crossVectors(right, forward).normalize();
  const tanY = Math.tan((fovDeg * Math.PI) / 360) / margin;
  const tanX = tanY * Math.max(aspect, 1e-3);

  let dist = 0;
  const v = new THREE.Vector3();
  for (let i = 0; i < 8; i++) {
    v.set(
      i & 1 ? box.max.x : box.min.x,
      i & 2 ? box.max.y : box.min.y,
      i & 4 ? box.max.z : box.min.z,
    ).sub(center);
    const x = v.dot(right);
    const y = v.dot(up);
    const z = v.dot(forward);
    dist = Math.max(dist, Math.abs(y) / tanY - z, Math.abs(x) / tanX - z);
  }
  // 退化包围盒（单点）时给一个非零距离，避免相机贴在模型上
  return Math.max(dist, 1e-3);
}

/**
 * 默认视角的「相机 → 目标」方向。
 *
 * 相机因此落在模型中心的 (-1,-1,-1) 一侧，也就是 **+x +y +z 那一侧**——
 * 与改造前的取景位置一致。方向写反了相机就会跑到模型的另一侧：正面的三角形
 * 会被背面剔除，界面上看到的是**一片空白**（实测就是这样把预览画没的）。
 */
export function defaultViewDir(): THREE.Vector3 {
  return new THREE.Vector3(-1, -1, -1).normalize();
}

/** 由预设角度（偏航、俯角，单位为度）求「相机 → 目标」的方向向量 */
export function presetDir(name: string): THREE.Vector3 {
  const angles = VIEW_PRESETS[name] ?? VIEW_PRESETS.front;
  const [yawDeg, pitchDeg] = angles;
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  return new THREE.Vector3(
    Math.cos(pitch) * Math.sin(yaw),
    -Math.sin(pitch),
    Math.cos(pitch) * Math.cos(yaw),
  );
}

/**
 * 把相机摆到「整个网格装进视口」的位置。
 *
 * 保持调用方给的视线方向，因此换视角后再自适应也不会把用户的视角掰回默认。
 */
export function frameBox(
  camera: THREE.PerspectiveCamera,
  box: THREE.Box3,
  dir: THREE.Vector3,
) {
  const center = box.getCenter(new THREE.Vector3());
  const dist = fitDistance(box, dir, camera.fov, camera.aspect);
  camera.position.copy(center).addScaledVector(dir.clone().normalize(), -dist);
  camera.lookAt(center);
  // 近裁剪面按距离给，缩放时不会切掉模型；远裁剪面留足余量
  camera.near = Math.max(dist / 100, 0.02);
  camera.far = Math.max(dist * 12, 100);
  camera.updateProjectionMatrix();
  return { center, dist };
}

/** 把相机拉到能完整看到整个网格的位置（默认视角） */
export function fitCamera(camera: THREE.PerspectiveCamera, group: THREE.Group) {
  const box = new THREE.Box3().setFromObject(group);
  if (box.isEmpty()) {
    // 空场景：放一个固定位置，避免 Box3 给出 NaN 把相机弄成不可用状态
    camera.position.set(12, 12, 12);
    camera.lookAt(0, 0, 0);
    camera.near = 0.1;
    camera.far = 1000;
    camera.updateProjectionMatrix();
    return;
  }
  frameBox(camera, box, defaultViewDir());
}

/**
 * 按预设视角摆放相机。
 *
 * 与界面上的「正面 / 左侧 45° / 右侧 45° / 背面」使用同一套角度定义，
 * 导出结果才会和用户在界面上看到的一致。
 */
export function applyPreset(
  camera: THREE.PerspectiveCamera,
  group: THREE.Group,
  name: string,
) {
  const box = new THREE.Box3().setFromObject(group);
  if (box.isEmpty()) {
    fitCamera(camera, group);
    return;
  }
  frameBox(camera, box, presetDir(name));
}

/**
 * 滚轮缩放的距离区间，按自适应后的基准距离定出来。
 *
 * 必须跟着模型大小走：一个 3 格高的小机器和一个 50 格长的大机器，
 * 用同一组绝对距离必然一边贴脸一边成点。
 */
export function zoomLimits(fitDist: number): { min: number; max: number } {
  return {
    min: Math.max(fitDist * 0.06, 0.05),
    max: Math.max(fitDist * 6, 1),
  };
}

/**
 * 按当前相机距离更新近/远裁剪面。
 *
 * OrbitControls 只管位置，**不管裁剪面**：取景时把 near 设成 dist/100 之后，
 * 用滚轮一直拉近就会穿进近裁剪面，模型被整片切掉——表现是「缩放几下画面就
 * 空了」。远裁剪面同理，拉远到头会把模型裁没。因此距离一变就要跟着更新。
 *
 * 返回是否真的变了，调用方据此决定要不要重绘。
 */
export function syncClipPlanes(
  camera: THREE.PerspectiveCamera,
  target: THREE.Vector3,
): boolean {
  const dist = camera.position.distanceTo(target);
  if (!Number.isFinite(dist) || dist <= 0) return false;
  const near = Math.max(dist / 100, 0.02);
  const far = Math.max(dist * 12, 100);
  if (near === camera.near && far === camera.far) return false;
  camera.near = near;
  camera.far = far;
  camera.updateProjectionMatrix();
  return true;
}

/** 导出允许的边长。与界面的下拉选项一致。 */
export const EXPORT_SIZES = [512, 1024, 2048, 3072] as const;

/**
 * 离屏渲染器：把任意场景数据渲染成透明背景的 PNG。
 *
 * 复用同一个 WebGL 上下文完成一次批量导出，而不是每个文件新建一个——
 * 浏览器同时可用的上下文数量有限（通常 16 个），逐个新建却不释放会
 * 在中途开始失败。用完由调用方 dispose()。
 */
export class OffscreenRenderer {
  private renderer: THREE.WebGLRenderer;
  private scene: THREE.Scene;
  private camera: THREE.PerspectiveCamera;

  constructor() {
    this.renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: true,
      // 读像素前不能让浏览器丢弃绘制缓冲，否则导出得到空白图
      preserveDrawingBuffer: true,
    });
    this.renderer.setPixelRatio(1);
    this.renderer.setClearColor(0x000000, 0);
    this.renderer.setClearAlpha(0);
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
  }

  /** 渲染一个场景并返回 PNG 字节。view 为视角预设名。 */
  async renderPng(data: SceneData, size: number, view = "front"): Promise<Uint8Array> {
    const built = buildSceneGroup(data);
    try {
      this.scene.add(built.group);
      // 正方形成像，宽高比固定为 1
      this.renderer.setSize(size, size, false);
      this.camera.aspect = 1;
      applyPreset(this.camera, built.group, view);
      this.renderer.render(this.scene, this.camera);

      const blob: Blob = await new Promise((resolve, reject) => {
        this.renderer.domElement.toBlob(
          (b) => (b ? resolve(b) : reject(new Error(t("export.canvasEmpty")))),
          "image/png",
        );
      });
      return new Uint8Array(await blob.arrayBuffer());
    } finally {
      this.scene.remove(built.group);
      built.dispose();
    }
  }

  dispose() {
    this.renderer.dispose();
    this.renderer.forceContextLoss();
  }
}
