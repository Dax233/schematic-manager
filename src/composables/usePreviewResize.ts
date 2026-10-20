/**
 * 预览面板高度的拖动调整。
 *
 * 预览面板（3D 视图 + 信息列 + 控制条）与下方页签区（信息 / 方块 / 实体）
 * 之间有一条水平分隔条：往上拖让预览占更多高度，往下拖把空间让给页签区。
 *
 * 舞台尺寸由这里给出的高度反算，而不是跟着窗口变：窗口缩放时舞台保持原样，
 * 只有用户主动拖动才会改变它——「界面不自行变化」与「允许用户调整」这两点
 * 因此可以同时成立。
 */
import { computed } from "vue";
import { usePanelResize } from "./usePanelResize";

/** 舞台的宽高基准。取自用户截图里标注的红框：398×344 CSS。 */
const STAGE_BASE_W = 398;
const STAGE_BASE_H = 344;

/** 预览面板在默认尺寸下的高度。
 *
 * 与原先写死的值一致：预览行 360（舞台 344 + 上下内边距 16）+ 控制条 40
 * + 分隔线 1。
 */
export const PREVIEW_DEFAULT_H = 401;

/** 面板高度的下限：舞台还要看得见，且控制条不能压到舞台上。
 *
 * 舞台按比例缩到不低于 20% 时仍有预览价值（398×0.2 已小到无意义，
 * 但再往下的高度会让控制条换行后压住舞台），因此取 200。
 */
export const PREVIEW_MIN_H = 200;

/** 面板高度的上限。窗口再高也不该让预览区独占整个详情区：
 *  至少给页签区留出 160px（统计条加一行表格）。 */
export const PREVIEW_MAX_H = 2000;

export function usePreviewResize() {
  const panel = usePanelResize({
    axis: "y",
    defaultSize: PREVIEW_DEFAULT_H,
    min: PREVIEW_MIN_H,
    max: PREVIEW_MAX_H,
    // 分隔条在面板下方，指针往上拖应当让面板变高
    reverse: true,
    // 详情区高度 = 窗口高 − 标题栏与工具栏占位，页签区还要留 160
    maxByWindow: () => window.innerHeight - 199 - 160,
  });

  /** 舞台按比例跟着面板高度缩放。
   *
   * 严格按 398:344 换算，不做单独的高度裁剪：比例一变，3D 视图的取景就会
   * 偏（相机是垂直 FOV），画面还会被拉扁。默认高度下正好还原 398×344。
   */
  const stage = computed(() => {
    // 面板高度里扣掉上下内边距 16、控制条 40、分隔线 1，剩下的就是舞台高度
    const h = Math.max(Math.round(panel.size.value - 57), 40);
    const w = Math.round((h * STAGE_BASE_W) / STAGE_BASE_H);
    return { w, h };
  });

  return {
    previewHeight: panel.size,
    previewStage: stage,
    previewResizing: panel.resizing,
    clampPreviewHeight: panel.clamp,
    startPreviewResize: panel.startResize,
    togglePreviewHeight: panel.toggleSize,
    onPreviewSplitterKey: panel.onSplitterKey,
  };
}
