import { defineConfig, type UserConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

// Tauri 开发时前端跑在固定端口，构建产物输出到 dist 供 Tauri 打包
export default defineConfig((): UserConfig => ({
  plugins: [vue()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    minify: "esbuild",
    sourcemap: false,
  },
  css: {
    preprocessorOptions: {
      scss: { api: "modern-compiler" },
    },
  },
}));
