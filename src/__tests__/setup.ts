/**
 * 测试环境补齐。
 *
 * jsdom 没有实现若干浏览器 API，而 Arco 组件与 Vue 的挂载过程会用到它们。
 * 这里补上最小可用的替身——**只补测试环境缺的，不改产品代码**，
 * 因此不会掩盖真实浏览器里的问题。
 */

// Arco 的响应式栅格依赖 matchMedia
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia;
}

// 部分组件会观察尺寸变化
if (typeof window !== "undefined" && !("ResizeObserver" in window)) {
  class ResizeObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  (window as unknown as { ResizeObserver: unknown }).ResizeObserver =
    ResizeObserverStub;
}

// 懒加载相关的观察者
if (typeof window !== "undefined" && !("IntersectionObserver" in window)) {
  class IntersectionObserverStub {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  }
  (window as unknown as { IntersectionObserver: unknown }).IntersectionObserver =
    IntersectionObserverStub;
}

// 指针事件：jsdom 没有 PointerEvent，而目录栏的拖动条用 pointerdown /
// pointermove / pointerup 实现。缺了它，拖动相关的测试会直接抛
// 「PointerEvent is not defined」，等于测不到拖动逻辑。
//
// 用 MouseEvent 派生：拖动只用到 clientX / button / buttons，
// 这些 MouseEvent 都有，构造出来的事件同样能被组件的监听器收到。
if (typeof window !== "undefined" && !("PointerEvent" in window)) {
  class PointerEventStub extends MouseEvent {
    pointerId: number;
    pointerType: string;
    isPrimary: boolean;

    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
      this.pointerType = init.pointerType ?? "mouse";
      this.isPrimary = init.isPrimary ?? true;
    }
  }
  (window as unknown as { PointerEvent: unknown }).PointerEvent = PointerEventStub;
  (globalThis as unknown as { PointerEvent: unknown }).PointerEvent = PointerEventStub;
}
