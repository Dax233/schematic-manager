import { describe, expect, it } from "vitest";
import * as THREE from "three";
import { gizmoDots } from "../render/view_gizmo";

/** 取某根轴的小球 */
function dotOf(orientation: THREE.Quaternion | null, axis: string) {
  const dots = gizmoDots(orientation, 100);
  const found = dots.find((d) => d.axis === axis);
  if (!found) throw new Error(`没有 ${axis} 这枚小球`);
  return found;
}

describe("方向标的坐标换算", () => {
  it("没有朝向时按单位基投影", () => {
    // 相机未就绪时不该抛错，也不该给出 NaN 坐标
    const dots = gizmoDots(null, 100);
    expect(dots.length, "应有六个轴向").toBe(6);
    for (const d of dots) {
      expect(Number.isFinite(d.x + d.y + d.depth), `${d.axis} 坐标应有限`).toBe(true);
    }
  });

  it("相机未旋转时 +x 在右、+y 在上、+z 朝向观察者", () => {
    // 单位四元数表示相机没有转动，此时世界轴就是相机轴
    const q = new THREE.Quaternion();
    const px = dotOf(q, "+x");
    expect(px.x, "+x 应在屏幕右侧").toBeGreaterThan(0);
    expect(px.y, "+x 纵坐标应为 0").toBeCloseTo(0, 5);

    const py = dotOf(q, "+y");
    // SVG 的 y 向下，屏幕「上方」对应负值
    expect(py.y, "+y 应在屏幕上方（y 为负）").toBeLessThan(0);
    expect(py.x, "+y 横坐标应为 0").toBeCloseTo(0, 5);

    const pz = dotOf(q, "+z");
    expect(pz.depth, "+z 应朝向观察者").toBeGreaterThan(0);
  });

  it("相机绕 Y 轴转 90° 后，+x 转到朝向观察者那一侧", () => {
    // 这是「方向标会不会跟着转」的核心：转动相机后，屏幕上的轴必须跟着换
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(0, 1, 0),
      Math.PI / 2,
    );
    const px = dotOf(q, "+x");
    expect(px.depth, "转 90° 后 +x 应朝向观察者").toBeGreaterThan(0.9);
    expect(px.x, "+x 此时横向分量应接近 0").toBeCloseTo(0, 3);
    // -z 会转到屏幕右侧
    const nz = dotOf(q, "-z");
    expect(nz.x, "-z 应转到屏幕右侧").toBeGreaterThan(90);
  });

  it("俯视 90° 时 +y 朝向观察者、+z 落在屏幕下方", () => {
    // 顶视角是常用视角，轴的位置必须符合直觉：从上往下看时，+z 指向屏幕下方
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(1, 0, 0),
      -Math.PI / 2,
    );
    const py = dotOf(q, "+y");
    expect(py.depth, "俯视时 +y 应朝向观察者").toBeGreaterThan(0.9);
    const pz = dotOf(q, "+z");
    expect(pz.y, "俯视时 +z 应落在屏幕下方（y 为正）").toBeGreaterThan(90);
  });

  it("按远近排序，越远的越靠前绘制", () => {
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(1, 0, 0),
      -Math.PI / 2,
    );
    const dots = gizmoDots(q, 100);
    for (let i = 1; i < dots.length; i++) {
      expect(
        dots[i].depth,
        `第 ${i} 个的 depth 不应小于前一个`,
      ).toBeGreaterThanOrEqual(dots[i - 1].depth);
    }
  });

  it("同一根轴的正负两端落在相反位置、深度也相反", () => {
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(0, 1, 0),
      Math.PI / 4,
    );
    for (const [pos, neg] of [
      ["+x", "-x"],
      ["+y", "-y"],
      ["+z", "-z"],
    ]) {
      const a = dotOf(q, pos);
      const b = dotOf(q, neg);
      expect(a.x, `${pos} 与 ${neg} 的 x 应相反`).toBeCloseTo(-b.x, 5);
      expect(a.y, `${pos} 与 ${neg} 的 y 应相反`).toBeCloseTo(-b.y, 5);
      expect(a.depth, `${pos} 与 ${neg} 的深度应相反`).toBeCloseTo(-b.depth, 5);
    }
  });
});
