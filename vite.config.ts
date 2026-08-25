import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 桌面端前端：固定 5173 端口（tauri.conf.json devUrl 对应），
// 环境变量前缀带 TAURI_，构建目标取 WebView2 基线。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
    outDir: "dist",
  },
});
