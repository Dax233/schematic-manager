import { describe, expect, it } from "vitest";
import * as THREE from "three";
import {
  applyPreset,
  buildSceneGroup,
  EXPORT_SIZES,
  fitCamera,
  VIEW_PRESETS,
} from "../render/scene3d";
import type { SceneData } from "../types";

/** 一个 1×1×1 的立方体场景，六面齐全。
 *
 * 角点顺序**照抄** geometry.rs 的 build_face_corners（from=(0,0,0)、to=(1,1,1)），
 * 不能自己随手排：这套顺序是 Minecraft 模型规范定的（它同时决定 UV 方向），
 * 而绕序正确与否恰好取决于「角点顺序 × 索引规则」两者。
 * 实测手写顺序会让 Up 面的法线变成朝内，从而把下面那条绕序断言带偏。
 */
function cubeScene(): SceneData {
  const faces: Array<[number, number, number][]> = [
    [[0, 0, 1], [1, 0, 1], [1, 0, 0], [0, 0, 0]], // Down
    [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1]], // Up
    [[1, 1, 0], [0, 1, 0], [0, 0, 0], [1, 0, 0]], // North
    [[0, 1, 1], [1, 1, 1], [1, 0, 1], [0, 0, 1]], // South
    [[0, 1, 0], [0, 1, 1], [0, 0, 1], [0, 0, 0]], // West
    [[1, 1, 1], [1, 1, 0], [1, 0, 0], [1, 0, 1]], // East
  ];
  const positions: number[] = [];
  const uvs: number[] = [];
  const colors: number[] = [];
  const indices: number[] = [];
  for (const f of faces) {
    const base = positions.length / 3;
    f.forEach((p, k) => {
      positions.push(p[0], p[1], p[2]);
      uvs.push(k === 0 || k === 3 ? 0 : 16, k < 2 ? 0 : 16);
      colors.push(1, 1, 1);
    });
    // 与后端 mesh::build 相同的规则：0-2-1 / 0-3-2
    indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }
  return {
    batches: [
      {
        texture: 0,
        positions,
        uvs,
        colors,
        indices,
        textureWidth: 2,
        textureHeight: 2,
        textureRgba: new Array(2 * 2 * 4).fill(255),
        alphaMode: "opaque" as const,
      },
    ],
    totalVertices: positions.length / 3,
    totalTriangles: indices.length / 3,
    enclosing: [1, 1, 1],
    origin: [0, 0, 0],
    mcVersionName: "1.20.1",
    dataVersion: 3465,
    unresolvedBlocks: 0,
  };
}

/** 直接读取 BufferAttribute 的第 i 个顶点，避免依赖内部字段名 */
function vertex(geo: THREE.BufferGeometry, i: number): [number, number, number] {
  const a = geo.getAttribute("position");
  return [a.getX(i), a.getY(i), a.getZ(i)];
}

/** 按面的 0-2-1 / 0-3-2 索引取几何法线（与外向判定同源） */
function triNormal(geo: THREE.BufferGeometry, i0: number, i1: number, i2: number) {
  const p0 = new THREE.Vector3(...vertex(geo, i0));
  const p1 = new THREE.Vector3(...vertex(geo, i1));
  const p2 = new THREE.Vector3(...vertex(geo, i2));
  return new THREE.Vector3().subVectors(p1, p0).cross(new THREE.Vector3().subVectors(p2, p0));
}


/** 透明类型必须决定材质参数，否则染色玻璃会被 alphaTest 挖出窟窿。
 *
 * 三种类型的差别不是风格问题：
 * - opaque 不透明，正常写深度；
 * - cutout（树叶/草）用 alphaTest 丢掉全透明像素，其余照常写深度；
 * - translucent（染色玻璃/水）必须混合且不写深度，**不能用 alphaTest**，
 *   否则它的半透明像素会被整片丢弃。
 */
describe("按透明类型选择材质", () => {
  /** 造一个只有单批次、指定透明类型的场景 */
  function sceneWith(alphaMode: "opaque" | "cutout" | "translucent"): SceneData {
    const s = cubeScene();
    s.batches[0].alphaMode = alphaMode;
    return s;
  }

  function materialOf(alphaMode: "opaque" | "cutout" | "translucent"): THREE.MeshBasicMaterial {
    const { group, dispose } = buildSceneGroup(sceneWith(alphaMode));
    const mesh = group.children[0] as THREE.Mesh;
    const mat = mesh.material as THREE.MeshBasicMaterial;
    // 断言后再释放，避免 WebGL 资源泄漏
    const snapshot = {
      transparent: mat.transparent,
      alphaTest: mat.alphaTest,
      depthWrite: mat.depthWrite,
    };
    dispose();
    // 用副本返回，避免调用方拿着已释放的材质
    return snapshot as unknown as THREE.MeshBasicMaterial;
  }

  it("染色玻璃等半透明贴图必须用混合，不能被 alphaTest 丢弃像素", () => {
    const m = materialOf("translucent");
    expect(m.transparent, "半透明贴图必须开启混合").toBe(true);
    expect(m.alphaTest, "半透明贴图不能用 alphaTest，否则半透明像素被丢弃").toBe(0);
    expect(m.depthWrite, "半透明面不应写深度，否则会挡掉后面的几何").toBe(false);
  });

  it("树叶等镂空贴图用 alphaTest，并保留深度写入", () => {
    const m = materialOf("cutout");
    expect(m.alphaTest, "镂空贴图应丢弃全透明像素").toBe(0.5);
    expect(m.depthWrite, "镂空部分必须写深度，否则后面的方块会透出来").toBe(true);
  });

  it("不透明贴图既不用混合也不用 alphaTest", () => {
    const m = materialOf("opaque");
    expect(m.transparent).toBe(false);
    expect(m.alphaTest).toBe(0);
    expect(m.depthWrite, "不透明方块必须写深度才能正确互相遮挡").toBe(true);
  });
});

describe("scene3d 网格构建", () => {
  it("每个方块面都朝向外部", () => {
    // 这正是「透视方块」的判据：材质用 FrontSide，法线朝内的面从外面看
    // 会被剔除，表现为看穿方块。这里对六个面逐一核对法线与朝外方向同向。
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      expect(group.children.length).toBe(1);
      const mesh = group.children[0] as THREE.Mesh;
      const geo = mesh.geometry as THREE.BufferGeometry;
      const index = geo.getIndex()!;
      // 立方体中心，用于判断法线是否背离中心
      const center = new THREE.Vector3(0.5, 0.5, 0.5);

      // 六个面 × 2 个三角形 × 3 个索引 = 36 个索引
      expect(index.count, "六个面 -> 36 个索引").toBe(36);
      for (let t = 0; t < index.count; t += 3) {
        const n = triNormal(geo, index.getX(t), index.getX(t + 1), index.getX(t + 2));
        // 三角形重心
        const c = new THREE.Vector3()
          .add(new THREE.Vector3(...vertex(geo, index.getX(t))))
          .add(new THREE.Vector3(...vertex(geo, index.getX(t + 1))))
          .add(new THREE.Vector3(...vertex(geo, index.getX(t + 2))))
          .divideScalar(3);
        const away = new THREE.Vector3().subVectors(c, center);
        // 立方体是不封闭的「空心壳」，重心到中心的连线与法线应大致同向。
        // 用点积判正负即可：反向即为「朝内」，会被 FrontSide 剔除。
        expect(
          n.dot(away),
          `第 ${t / 3} 个三角形的法线 ${n.toArray()} 指向内部（重心 ${c.toArray()}）→ 会被 FrontSide 剔除`,
        ).toBeGreaterThan(0);
      }
    } finally {
      dispose();
    }
  });

  it("顶点位置减去 origin，使模型居中", () => {
    const data = cubeScene();
    // 把原点挪到 (10, 20, 30)：顶点应相应左移，范围回到 0..1
    data.origin = [10, 20, 30];
    data.batches[0].positions = data.batches[0].positions.map((v, i) =>
      i % 3 === 0 ? v + 10 : i % 3 === 1 ? v + 20 : v + 30,
    );
    const { group, dispose } = buildSceneGroup(data);
    try {
      const geo = (group.children[0] as THREE.Mesh).geometry as THREE.BufferGeometry;
      geo.computeBoundingBox();
      const box = geo.boundingBox!;
      expect(box.min.x).toBeCloseTo(0);
      expect(box.min.y).toBeCloseTo(0);
      expect(box.min.z).toBeCloseTo(0);
      expect(box.max.x).toBeCloseTo(1);
      expect(box.max.y).toBeCloseTo(1);
      expect(box.max.z).toBeCloseTo(1);
    } finally {
      dispose();
    }
  });

  it("dispose 后不留子对象，避免反复切换投影累积显存", () => {
    const built = buildSceneGroup(cubeScene());
    expect(built.group.children.length).toBe(1);
    built.dispose();
    expect(built.group.children.length, "dispose 应清空子对象").toBe(0);
  });

  it("空场景不产生网格，也不会抛错", () => {
    const data = cubeScene();
    data.batches = [];
    const built = buildSceneGroup(data);
    try {
      expect(built.group.children.length).toBe(0);
    } finally {
      built.dispose();
    }
  });
});

describe("相机取景", () => {
  it("四个预设都能把相机放在模型外侧且看得到模型", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      for (const name of Object.keys(VIEW_PRESETS)) {
        const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
        applyPreset(cam, group, name);
        // 相机应在模型之外（避免「贴到面上」看到一片同色）
        const box = new THREE.Box3().setFromObject(group);
        const center = box.getCenter(new THREE.Vector3());
        const dist = cam.position.distanceTo(center);
        const radius = box.getSize(new THREE.Vector3()).length() * 0.5;
        expect(dist, `${name}: 相机距离 ${dist} 应大于模型半径 ${radius}`).toBeGreaterThan(radius);
        // 相机朝向应指向模型中心附近，而不是背对
        const dir = cam.getWorldDirection(new THREE.Vector3());
        const toCenter = new THREE.Vector3().subVectors(center, cam.position).normalize();
        expect(dir.dot(toCenter), `${name}: 相机应朝向模型`).toBeGreaterThan(0.99);
        // 近远平面必须包住模型，否则会被裁掉
        expect(cam.near, `${name}: near 应为正`).toBeGreaterThan(0);
        expect(cam.far, `${name}: far 应大于到模型的近端距离`).toBeGreaterThan(dist - radius);
      }
    } finally {
      dispose();
    }
  });

  it("四个预设互不相同，且 front 与 back 相对", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const pos = (name: string) => {
        const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
        applyPreset(cam, group, name);
        return cam.position.clone();
      };
      const f = pos("front");
      const b = pos("back");
      // front 与 back 应在模型两侧：各自减去中心后方向相反
      const box = new THREE.Box3().setFromObject(group);
      const c = box.getCenter(new THREE.Vector3());
      const df = f.clone().sub(c).normalize();
      const db = b.clone().sub(c).normalize();
      expect(df.dot(db), "front 与 back 应在相反方向").toBeLessThan(-0.5);
      expect(pos("left45").equals(pos("right45")), "左右 45° 视角不应重合").toBe(false);
    } finally {
      dispose();
    }
  });

  it("空场景时相机落到可用位置而不是 NaN", () => {
    const empty = new THREE.Group();
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    applyPreset(cam, empty, "front");
    // 用 Number.isFinite：chai 没有 toBeFinite
    expect(Number.isFinite(cam.position.x)).toBe(true);
    expect(Number.isFinite(cam.position.y)).toBe(true);
    expect(Number.isFinite(cam.position.z)).toBe(true);
    expect(cam.near).toBeGreaterThan(0);
  });
});

describe("导出尺寸", () => {
  it("提供 512 / 1024 / 2048 / 3072 四档", () => {
    expect([...EXPORT_SIZES]).toEqual([512, 1024, 2048, 3072]);
  });
});

/** 相机必须对准结构中心，否则取景会偏、旋转也会绕错点。 */
describe("相机取景", () => {
  /** 造一个偏移到指定位置的立方体，用来验证取景是否跟着结构走 */
  function cubeAt(x: number, y: number, z: number): THREE.Group {
    const { group } = buildSceneGroup(cubeScene());
    group.position.set(x, y, z);
    group.updateMatrixWorld(true);
    return group;
  }

  function newCamera(): THREE.PerspectiveCamera {
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    cam.updateMatrixWorld(true);
    return cam;
  }

  it("取景后相机朝向结构中心", () => {
    const group = cubeAt(100, 200, 300);
    const cam = newCamera();
    fitCamera(cam, group);

    const box = new THREE.Box3().setFromObject(group);
    const center = box.getCenter(new THREE.Vector3());
    // lookAt 已把朝向设好，但方向要在 updateMatrixWorld 之后才反映到矩阵里
    cam.updateMatrixWorld(true);
    const dir = cam.getWorldDirection(new THREE.Vector3());
    const toCenter = center.clone().sub(cam.position).normalize();
    expect(
      dir.dot(toCenter),
      "相机朝向应与「相机 -> 结构中心」几乎一致；偏差大说明取景没对准结构",
    ).toBeGreaterThan(0.999);
  });

  it("结构离原点越远，相机就跟着移到越远", () => {
    // 这条防的是「只在原点取景」：那样结构放在远处时会飞出画面
    const near = cubeAt(0, 0, 0);
    const far = cubeAt(500, 500, 500);
    const camNear = newCamera();
    const camFar = newCamera();
    fitCamera(camNear, near);
    fitCamera(camFar, far);

    expect(
      camFar.position.length(),
      "结构移到 (500,500,500) 后相机也必须跟着移过去",
    ).toBeGreaterThan(camNear.position.length() + 100);
  });

  it("四个预设视角都对准结构中心", () => {
    const group = cubeAt(20, 30, 40);
    const box = new THREE.Box3().setFromObject(group);
    const center = box.getCenter(new THREE.Vector3());

    for (const name of Object.keys(VIEW_PRESETS)) {
      const cam = newCamera();
      applyPreset(cam, group, name);
      cam.updateMatrixWorld(true);
      const dir = cam.getWorldDirection(new THREE.Vector3());
      const toCenter = center.clone().sub(cam.position).normalize();
      expect(
        dir.dot(toCenter),
        `预设视角「${name}」没有对准结构中心`,
      ).toBeGreaterThan(0.999);
    }
  });

  it("空结构不会把相机弄成 NaN", () => {
    const empty = new THREE.Group();
    const cam = newCamera();
    fitCamera(cam, empty);
    // Box3 为空时若不特判，radius/dist 会变成 NaN 并把相机弄坏
    expect(Number.isFinite(cam.position.x), "相机位置不应为 NaN").toBe(true);
    expect(Number.isFinite(cam.position.y)).toBe(true);
    expect(Number.isFinite(cam.position.z)).toBe(true);
  });
});
