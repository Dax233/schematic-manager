/**
 * 方向标的坐标换算：把世界坐标的三个轴按相机朝向投到屏幕平面上。
 *
 * 抽成独立模块的原因：这段几何换算容易出错（旋转方向搞反、y 轴上下颠倒），
 * 而错了以后界面上只是「小球位置不太对」，很难一眼看出。独立后才能直接用
 * 单测把「正视角下哪个轴该出现在哪」钉死。
 *
 * 注意屏幕坐标系的原点在中心、y 向下；调用方按需自行平移。
 */
import * as THREE from "three";
import { GIZMO_AXES } from "./scene3d";

/** 方向标上的一枚端点小球 */
export interface GizmoDot {
  /** 视角预设的键，点击时用它切视角 */
  key: string;
  /** 轴向标识，如 "+x" */
  axis: string;
  /** 屏幕横坐标（右为正） */
  x: number;
  /** 屏幕纵坐标（下为正，与 SVG 一致） */
  y: number;
  /** 该轴在相机坐标系里的朝向分量：正数表示朝向观察者 */
  depth: number;
}

/** 由轴向标识得到世界坐标里的单位向量 */
export function axisVector(axis: string): THREE.Vector3 {
  return new THREE.Vector3(
    axis === "+x" ? 1 : axis === "-x" ? -1 : 0,
    axis === "+y" ? 1 : axis === "-y" ? -1 : 0,
    axis === "+z" ? 1 : axis === "-z" ? -1 : 0,
  );
}

/**
 * 把六个轴向按相机朝向投到屏幕上。
 *
 * 做法：世界向量乘相机旋转的逆，得到它在相机坐标系里的分量。相机的
 * 局部 +x 是屏幕向右、+y 是屏幕向上，因此 local.x 直接是横坐标，
 * local.y 取负才是 SVG 的纵坐标（SVG 的 y 向下）。
 *
 * 返回结果按 depth 升序（越远的越靠前），调用方顺序绘制即可得到正确遮挡。
 */
export function gizmoDots(
  orientation: THREE.Quaternion | null,
  radius: number,
): GizmoDot[] {
  const inv = orientation ? orientation.clone().invert() : null;
  const dots = GIZMO_AXES.map((entry) => {
    const local = axisVector(entry.axis);
    if (inv) local.applyQuaternion(inv);
    return {
      key: entry.key,
      axis: entry.axis,
      x: local.x * radius,
      y: -local.y * radius,
      depth: local.z,
    };
  });
  return dots.sort((a, b) => a.depth - b.depth);
}
