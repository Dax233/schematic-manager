//! 由场景数据构建 three.js 网格，以及离屏渲染成 PNG。
//!
//! 抽成独立模块的原因：批量导出需要渲染**当前没有显示**的投影。
//! 若复用界面上的那个 viewer，就必须先把界面切到每个文件、
//! 等它渲染完再导出，过程既慢又会把用户正在看的画面反复冲掉。
//!
//! 因此这里提供一个独立的离屏渲染器；界面的 SceneViewer 复用其中的
//! 网格构建与取景逻辑，避免两处各写一份而逐渐不一致。

import * as THREE from "three";
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

    const mat = new THREE.MeshBasicMaterial({
      map: tex,
      vertexColors: true,
      // 后端给出的三角形绕序已使其法线朝外，因此正面渲染即可。
      // 用 DoubleSide 会同时画出方块内部的面，透视效果反而不对。
      side: THREE.FrontSide,
      transparent: true,
      // **不要**设 alphaTest。
      //
      // 染色玻璃这类贴图的 alpha 是半透明的（实测白色/黄色染色玻璃各 16×16
      // 里有 191 个像素的 alpha 落在 1..127），原本按逐方块取的第一张玻璃贴图
      // 首像素 alpha 为 163。alphaTest 会直接**丢弃**低于阈值的像素，
      // 而这些像素本应作为半透明画出来——结果是玻璃上被挖出大片窟窿/缺面。
      //
      // 完全透明的像素（alpha=0）由 transparent + 普通混合处理，
      // 不需要 alphaTest 也看不见，因此去掉它没有副作用。
      depthWrite: false,
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
  const size = box.getSize(new THREE.Vector3());
  const center = box.getCenter(new THREE.Vector3());
  const radius = Math.max(size.length() * 0.5, 1);
  const dist = radius / Math.sin((camera.fov * Math.PI) / 360);
  camera.position.copy(center).add(new THREE.Vector3(dist, dist, dist));
  camera.lookAt(center);
  camera.near = Math.max(dist / 100, 0.05);
  camera.far = dist * 10;
  camera.updateProjectionMatrix();
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
  const angles = VIEW_PRESETS[name] ?? VIEW_PRESETS.front;
  const box = new THREE.Box3().setFromObject(group);
  if (box.isEmpty()) {
    fitCamera(camera, group);
    return;
  }
  const center = box.getCenter(new THREE.Vector3());
  const radius = Math.max(box.getSize(new THREE.Vector3()).length() * 0.5, 1);
  const dist = radius / Math.sin((camera.fov * Math.PI) / 360);
  const [yawDeg, pitchDeg] = angles;
  const yaw = (yawDeg * Math.PI) / 180;
  const pitch = (pitchDeg * Math.PI) / 180;
  const dir = new THREE.Vector3(
    Math.cos(pitch) * Math.sin(yaw),
    -Math.sin(pitch),
    Math.cos(pitch) * Math.cos(yaw),
  );
  camera.position.copy(center).addScaledVector(dir, -dist);
  camera.lookAt(center);
  camera.near = Math.max(dist / 100, 0.05);
  camera.far = dist * 10;
  camera.updateProjectionMatrix();
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
          (b) => (b ? resolve(b) : reject(new Error("导出图像失败：画布为空"))),
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
