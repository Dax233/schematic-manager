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
