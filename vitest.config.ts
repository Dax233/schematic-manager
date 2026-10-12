import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

// 前端测试配置。
//
// 存在的理由：本项目此前只有 Rust 侧测试，前端的逻辑（调用哪个命令、
// 传什么参数、界面能否渲染）完全没有自动化保护。而「点击按钮没反应」
// 最常见的成因就是命令名或参数名写错——这类错误类型检查查不出，
// 只会在运行时表现为功能失效。
//
// 用 jsdom：挂载组件需要 DOM；Tauri 的 API 全部 mock，测试不依赖真实窗口。
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/__tests__/**/*.test.ts"],
    // 补齐 jsdom 缺少的浏览器 API（matchMedia 等），Arco 组件需要
    setupFiles: ["src/__tests__/setup.ts"],
    // Arco 组件在挂载时会输出较多警告，保留但不视为失败
    globals: false,
  },
  css: {
    preprocessorOptions: { scss: { api: "modern-compiler" } },
  },
});
