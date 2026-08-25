import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

// 注意：不要在这里全量注册 naive-ui（app.use(naive)）。
// 所有组件已在各 SFC 中按需显式导入；全量注册会让 dev 首屏
// 加载并解析整个组件库，是 `tauri dev` 启动长时间白屏的主因之一。
createApp(App).mount("#app");
