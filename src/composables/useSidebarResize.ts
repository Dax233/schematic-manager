/**
 * 左侧目录栏宽度的拖动调整。
 *
 * 独立成组合式函数的原因：这套逻辑只关心「一条分隔条如何改变一个宽度值」，
 * 与目录栏里显示什么内容无关；窗口 resize 监听、拖动过程中的全局指针监听
 * 都由它自己登记与清理，使用方只需要拿到宽度和分隔条事件处理器。
 *
 * 使用方式：`const { sidebarWidth, resizing, startResize, ... } = useSidebarResize()`，
 * 宽度值同时被界面状态记忆（store/uiState）读取与恢复。
 */
import { onBeforeUnmount, onMounted, ref } from "vue";

/** 名称列的默认宽度。取实测自然宽度（176px）的两倍。
 *
 * 名称是最需要宽度的列：投影文件名普遍较长，之前一律被省略号截断，
 * 只能靠悬停提示看全名。加倍后同屏能直接读出更多字符。
 *
 * 声明在这里而不是文件列表模块里：目录栏默认宽度要用它算出
 * 「能容下全部列」的宽度，文件列表的列定义也引用它。
 */
export const NAME_COL_W = 352;

/** 左侧目录栏宽度（像素）的合法区间。
 *
 * 太窄会让「名称/制作版本/尺寸」三列挤在一起、太宽则把右侧预览与详情
 * 压得过小。
 */
export const SIDEBAR_MIN_W = 320;
export const SIDEBAR_MAX_W = 1200;
/** 各列默认宽度之和，含选择列与 1px 占位列。改列宽时要同步。 */
const SIDEBAR_FIT_W = 40 + NAME_COL_W + 90 + 110 + 73 + 1;
/** 目录栏默认宽度。
 *
 * 取参考截图的实测值：截图宽 2047 设备像素、缩放 1.25（由已知的标题栏 32px
 * 与工具栏 44px 反推），表头里的竖直分隔线在设备像素 52/492/605/742/833，
 * 换算成 CSS 像素是 41.6 / 393.6 / 484 / 593.6 / 666.4，
 * 即各列 40（选择）/ 352 / 90 / 110 / 73，合计正好 666。
 *
 * 仍保留 SIDEBAR_FIT_W 兜底：万一以后把某列调宽到放不下，
 * 目录栏跟着变宽，而不是把末列挤出可视区。 */
const SIDEBAR_DEFAULT_W = Math.max(666, SIDEBAR_FIT_W);

export function useSidebarResize() {
  /** 当前目录栏宽度。不写死在样式里：用户可拖动分隔条调整。 */
  const sidebarWidth = ref(SIDEBAR_DEFAULT_W);
  /** 是否正在拖动分隔条。拖动期间禁用过渡并屏蔽文本选择 */
  const resizing = ref(false);

  /// 夹到合法区间，避免拖动或窗口变化后得到不可用宽度
  function clampSidebar(w: number): number {
    const maxByWindow = Math.max(SIDEBAR_MIN_W, window.innerWidth - 360);
    return Math.min(Math.max(Math.round(w), SIDEBAR_MIN_W),
      Math.min(SIDEBAR_MAX_W, maxByWindow));
  }

  /// 从指针事件取横坐标并更新宽度
  function resizeTo(clientX: number) {
    sidebarWidth.value = clampSidebar(clientX);
  }

  function onSplitterMove(e: PointerEvent) {
    if (!resizing.value) return;
    resizeTo(e.clientX);
  }

  function stopResize() {
    if (!resizing.value) return;
    resizing.value = false;
    window.removeEventListener("pointermove", onSplitterMove);
    window.removeEventListener("pointerup", stopResize);
    window.removeEventListener("pointercancel", stopResize);
  }

  /** 开始拖动分隔条。
   *
   * 监听挂在 window 上而不是分隔条自身：指针移出那条 5px 宽的窄条后
   * 事件仍会继续送到 window，拖动才不会中途断掉。
   */
  function startResize(e: PointerEvent) {
    // 只响应左键，避免右键菜单等操作把界面拖乱
    if (e.button !== 0) return;
    e.preventDefault();
    resizing.value = true;
    window.addEventListener("pointermove", onSplitterMove);
    window.addEventListener("pointerup", stopResize);
    window.addEventListener("pointercancel", stopResize);
  }

  /** 双击分隔条：在默认宽度与最窄宽度之间切换，便于快速让出空间 */
  function toggleSidebarWidth() {
    sidebarWidth.value = sidebarWidth.value <= SIDEBAR_MIN_W + 8
      ? clampSidebar(SIDEBAR_DEFAULT_W)
      : SIDEBAR_MIN_W;
  }

  /** 键盘可用的分隔条：左右方向键调整宽度。
   *
   * 分隔条是 role="separator" 的可聚焦元素，必须能用键盘操作，
   * 只支持鼠标拖动会让键盘用户无法调整布局。
   */
  function onSplitterKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 8;
    if (e.key === "ArrowLeft") {
      sidebarWidth.value = clampSidebar(sidebarWidth.value - step);
    } else if (e.key === "ArrowRight") {
      sidebarWidth.value = clampSidebar(sidebarWidth.value + step);
    } else if (e.key === "Home") {
      sidebarWidth.value = SIDEBAR_MIN_W;
    } else if (e.key === "End") {
      sidebarWidth.value = clampSidebar(SIDEBAR_MAX_W);
    } else {
      return;
    }
    e.preventDefault();
  }

  /** 窗口变窄时把过宽的目录栏收回来，否则右侧会被挤没 */
  function onWindowResize() {
    const clamped = clampSidebar(sidebarWidth.value);
    if (clamped !== sidebarWidth.value) sidebarWidth.value = clamped;
  }

  onMounted(() => {
    window.addEventListener("resize", onWindowResize);
    onWindowResize();
  });

  // 组件卸载时摘掉 window 上的监听，避免拖动中关闭窗口而留下悬挂的监听
  onBeforeUnmount(() => {
    window.removeEventListener("resize", onWindowResize);
    stopResize();
  });

  return {
    sidebarWidth,
    resizing,
    clampSidebar,
    startResize,
    toggleSidebarWidth,
    onSplitterKey,
  };
}
