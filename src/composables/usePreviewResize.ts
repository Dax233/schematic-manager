/**
 * 预览面板高度的拖动调整。
 *
 * 预览面板（3D 视图 + 信息列 + 控制条）与下方页签区（信息 / 方块 / 实体）
 * 之间有一条水平分隔条：往上拖让预览占更多高度，往下拖把空间让给页签区。
 *
 * 舞台尺寸只由这里给出的高度决定，不跟随窗口变化：窗口缩放时舞台保持原样，
 * 只有用户主动拖动才会改变它——「界面不自行变化」与「允许用户调整」因此
 * 可以同时成立。
 */
import { computed, ref } from "vue";
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

/** 面板高度的下限：再矮就没有预览价值，且控制条会压到舞台上。 */
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

  /** 预览行**高度**的实测值，由组件用 ResizeObserver 写入。
   *
   * 不能用「面板高度 − 固定常数」推算舞台：控制条是 flex-wrap 的，
   * 预览区窄到放不下那一排控件时会折成两行，实际占用的高度从 40 变成 80，
   * 按常数算出来的舞台就会超出可用空间、被 overflow 裁掉。
   *
   * 只取高度不取宽度：宽度受舞台自身尺寸影响，用它反算舞台会形成回路。
   */
  const rowHeight = ref<number | null>(null);

  function measureRowHeight(h: number) {
    rowHeight.value = h > 0 ? h : null;
  }

  /** 舞台尺寸：按 398:344 等比取最大可用尺寸。
   *
   * 严格等比而不各自伸缩：比例一变，3D 视图的取景就会偏（相机是垂直 FOV），
   * 画面还会被拉扁。默认高度下正好还原 398×344。
   */
  const stage = computed(() => {
    // 拿不到实测值时（首帧、测试环境）按面板高度推算，
    // 扣掉上下内边距 16、控制条 40、分隔线 1
    const avail = rowHeight.value
      ? rowHeight.value - 16
      : Math.max(panel.size.value - 57, 40);
    const h = Math.round(Math.max(avail, 40));
    const w = Math.round((h * STAGE_BASE_W) / STAGE_BASE_H);
    return { w, h };
  });

  return {
    previewHeight: panel.size,
    previewStage: stage,
    previewRowHeight: rowHeight,
    measurePreviewRowHeight: measureRowHeight,
    previewResizing: panel.resizing,
    clampPreviewHeight: panel.clamp,
    startPreviewResize: panel.startResize,
    togglePreviewHeight: panel.toggleSize,
    onPreviewSplitterKey: panel.onSplitterKey,
  };
}
