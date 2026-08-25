import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 桌面端前端：固定 5173 端口（tauri.conf.json devUrl 对应），
// 环境变量前缀带 TAURI_，构建目标取 WebView2 基线。
// Vite 8：打包/压缩/依赖预打包已默认切换到 Rolldown + Oxc（不再用 esbuild/Rollup）。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  // 服务启动即预热依赖预打包：避免首次请求才发现 naive-ui 再触发
  // 「重新优化依赖 → 整页 reload」，表现为启动后白屏很久。
  // （Vite 7 = esbuild 预打包；Vite 8 Rolldown 同样支持 include。）
  optimizeDeps: {
    include: ["naive-ui", "@vueuse/core", "vue", "@tauri-apps/api", "@tauri-apps/plugin-dialog"],
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "chrome110",
    sourcemap: false,
    outDir: "dist",
  },
});
