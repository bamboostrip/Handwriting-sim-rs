//! 全局应用状态（reactive 单例）——对应原 Slint 版 main.rs 的全部 UI 逻辑：
//! 参数收集（buildParams ↔ collect_params）、防抖预览、翻页、预设、
//! 段落结构操作（分段/合并/对齐/缩进）、框选区域增删改、文档底图同步。
//!
//! 渲染代次守卫：renderSeq 只采纳最新一次请求结果，过期响应直接丢弃。

import { nextTick, reactive, watch } from "vue";
import { createDiscreteApi } from "naive-ui";
import { useDebounceFn } from "@vueuse/core";
import { api, assetUrl, dialogs } from "./api";
import type {
  SystemFontItem,
  UiHandwritingRole,
  UiParams,
  UiRegion,
  UiShape,
  UiShapeKind,
  UiTextRun,
  UpdateInfo,
} from "./types";

/** 当前版本号与开源仓库地址 */
export const APP_VERSION = "0.6.0";

export const PYTHON_REPO_URL = "https://github.com/bamboostrip/Handwriting-simulator";
export const RUST_REPO_URL = "https://github.com/bamboostrip/Handwriting-sim-rs";

const KEY_AUTO_CHECK = "handwritesim_auto_check";
const KEY_SKIPPED_VERSION = "handwritesim_skipped_version";
const KEY_THEME_PREFERENCE = "handwritesim_theme_preference";
const KEY_PANEL_COLLAPSE = "handwritesim_panel_collapse_v2";
const KEY_ONBOARDING_DONE = "handwritesim_onboarding_done";
const KEY_CHECKLIST_DISMISSED = "handwritesim_checklist_dismissed";
const KEY_HAS_EXPORTED = "handwritesim_has_exported";

function readStoredBool(key: string, fallback = false): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v === "true";
  } catch {
    return fallback;
  }
}

function writeStoredBool(key: string, value: boolean): void {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    // localStorage 不可用时静默降级
  }
}

export type ThemePreference = "auto" | "light" | "dark";

export function getStoredThemePreference(): ThemePreference {
  try {
    const v = localStorage.getItem(KEY_THEME_PREFERENCE);
    if (v === "light" || v === "dark" || v === "auto") return v;
    return "auto";
  } catch {
    return "auto";
  }
}

export function isAutoCheckEnabled(): boolean {
  try {
    const v = localStorage.getItem(KEY_AUTO_CHECK);
    return v === null ? true : v === "true";
  } catch {
    return true;
  }
}

export function getSkippedVersion(): string {
  try {
    return localStorage.getItem(KEY_SKIPPED_VERSION) || "";
  } catch {
    return "";
  }
}

/** 右栏阶段分组的展开记录（true = 折叠）；无记录返回 null（首次使用） */
export type PanelCollapseState = {
  content: boolean;
  style: boolean;
  layout: boolean;
  regions: boolean;
  /** 图形标注分组（旧记录可能没有该键，缺省按展开处理） */
  shapes?: boolean;
};

export function loadPanelCollapseState(): PanelCollapseState | null {
  try {
    const raw = localStorage.getItem(KEY_PANEL_COLLAPSE);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<PanelCollapseState>;
    return {
      content: v.content ?? false,
      style: v.style ?? true,
      layout: v.layout ?? true,
      regions: v.regions ?? true,
      // 图形标注是本版新增分组：旧记录没有该键时默认展开，让用户一眼看到新入口
      shapes: v.shapes ?? false,
    };
  } catch {
    return null;
  }
}

export function savePanelCollapseState(s: PanelCollapseState): void {
  try {
    localStorage.setItem(KEY_PANEL_COLLAPSE, JSON.stringify(s));
  } catch {
    // localStorage 不可用时静默降级为不记忆
  }
}


/** 组件树外的离散弹窗（store 层的阻断性错误提示用） */
const { dialog: appDialog, message: appMessage } = createDiscreteApi(["dialog", "message"]);

// ---------------------------------------------------------------- 数据模型与高亮角色定义

export interface WordHighlightColor {
  key: string;
  name: string;
  bg: string;
  color: string;
  darkBg: string;
  darkColor: string;
  icon: string;
}

export const WORD_HIGHLIGHT_MAP: Record<string, WordHighlightColor> = {
  yellow: {
    key: "yellow",
    name: "黄色",
    bg: "#fef08a",
    color: "#713f12",
    darkBg: "#713f12",
    darkColor: "#fef08a",
    icon: "🟨",
  },
  green: {
    key: "green",
    name: "绿色",
    bg: "#bbf7d0",
    color: "#14532d",
    darkBg: "#14532d",
    darkColor: "#bbf7d0",
    icon: "🟩",
  },
  cyan: {
    key: "cyan",
    name: "青色",
    bg: "#bae6fd",
    color: "#0c4a6e",
    darkBg: "#0c4a6e",
    darkColor: "#bae6fd",
    icon: "🟦",
  },
  magenta: {
    key: "magenta",
    name: "品红",
    bg: "#fbcfe8",
    color: "#831843",
    darkBg: "#831843",
    darkColor: "#fbcfe8",
    icon: "🟪",
  },
  blue: {
    key: "blue",
    name: "蓝色",
    bg: "#bfdbfe",
    color: "#1e3a8a",
    darkBg: "#1e3a8a",
    darkColor: "#bfdbfe",
    icon: "🟦",
  },
  red: {
    key: "red",
    name: "红色",
    bg: "#fecaca",
    color: "#7f1d1d",
    darkBg: "#7f1d1d",
    darkColor: "#fecaca",
    icon: "🟥",
  },
  darkBlue: {
    key: "darkBlue",
    name: "深蓝",
    bg: "#93c5fd",
    color: "#172554",
    darkBg: "#172554",
    darkColor: "#93c5fd",
    icon: "🟦",
  },
  darkCyan: {
    key: "darkCyan",
    name: "深青",
    bg: "#67e8f9",
    color: "#083344",
    darkBg: "#083344",
    darkColor: "#67e8f9",
    icon: "🟦",
  },
  darkGreen: {
    key: "darkGreen",
    name: "深绿",
    bg: "#86efac",
    color: "#052e16",
    darkBg: "#052e16",
    darkColor: "#86efac",
    icon: "🟩",
  },
  darkMagenta: {
    key: "darkMagenta",
    name: "深品红",
    bg: "#f472b6",
    color: "#500724",
    darkBg: "#500724",
    darkColor: "#f472b6",
    icon: "🟪",
  },
  darkRed: {
    key: "darkRed",
    name: "深红",
    bg: "#f87171",
    color: "#450a0a",
    darkBg: "#450a0a",
    darkColor: "#f87171",
    icon: "🟥",
  },
  darkYellow: {
    key: "darkYellow",
    name: "深黄",
    bg: "#fde047",
    color: "#422006",
    darkBg: "#422006",
    darkColor: "#fde047",
    icon: "🟨",
  },
  lightGray: {
    key: "lightGray",
    name: "浅灰",
    bg: "#e2e8f0",
    color: "#334155",
    darkBg: "#334155",
    darkColor: "#e2e8f0",
    icon: "⬛",
  },
  darkGray: {
    key: "darkGray",
    name: "深灰",
    bg: "#94a3b8",
    color: "#0f172a",
    darkBg: "#0f172a",
    darkColor: "#94a3b8",
    icon: "⬛",
  },
  black: {
    key: "black",
    name: "黑色",
    bg: "#334155",
    color: "#ffffff",
    darkBg: "#0f172a",
    darkColor: "#f8fafc",
    icon: "⬛",
  },
};

/** 查找高亮信息（不区分大小写及下划线/连字符） */
export function getHighlightInfo(key?: string | null): WordHighlightColor | null {
  if (!key) return null;
  const raw = key.trim();
  if (WORD_HIGHLIGHT_MAP[raw]) return WORD_HIGHLIGHT_MAP[raw];
  const norm = raw.toLowerCase().replace(/[-_\s]/g, "");
  if (norm === "pink") return WORD_HIGHLIGHT_MAP.magenta;
  if (norm === "gray" || norm === "grey") return WORD_HIGHLIGHT_MAP.lightGray;
  for (const k of Object.keys(WORD_HIGHLIGHT_MAP)) {
    if (k.toLowerCase() === norm) return WORD_HIGHLIGHT_MAP[k];
  }
  return null;
}

/** 获取角色的徽章样式信息（背景色、文本色、图标、名称） */
export function getRoleBadgeInfo(role: UiHandwritingRole, isDark = isDarkActive()) {
  if (role.id === 0) {
    return {
      label: "主字体",
      icon: "🖊️",
      color: isDark ? "#94a3b8" : "#64748b",
      bg: isDark ? "rgba(148, 163, 184, 0.18)" : "rgba(100, 116, 139, 0.12)",
      highlightName: "主字体",
    };
  }
  if (role.id === 1 || role.printed) {
    const hl = getHighlightInfo(role.highlight) || WORD_HIGHLIGHT_MAP.lightGray;
    return {
      label: role.name || "打印体",
      icon: "🖨️",
      color: isDark ? hl.darkColor : hl.color,
      bg: isDark ? hl.darkBg : hl.bg,
      highlightName: hl.name,
    };
  }

  // 1. 若角色指定了显式高亮
  const hl = getHighlightInfo(role.highlight);
  if (hl) {
    return {
      label: role.name,
      icon: hl.icon,
      color: isDark ? hl.darkColor : hl.color,
      bg: isDark ? hl.darkBg : hl.bg,
      highlightName: hl.name,
    };
  }

  // 2. 预设默认角色 ID (2:黄, 3:绿, 4:青, 5:品红)
  if (role.id === 2) {
    const y = WORD_HIGHLIGHT_MAP.yellow;
    return { label: role.name, icon: y.icon, color: isDark ? y.darkColor : y.color, bg: isDark ? y.darkBg : y.bg, highlightName: y.name };
  }
  if (role.id === 3) {
    const g = WORD_HIGHLIGHT_MAP.green;
    return { label: role.name, icon: g.icon, color: isDark ? g.darkColor : g.color, bg: isDark ? g.darkBg : g.bg, highlightName: g.name };
  }
  if (role.id === 4) {
    const c = WORD_HIGHLIGHT_MAP.cyan;
    return { label: role.name, icon: c.icon, color: isDark ? c.darkColor : c.color, bg: isDark ? c.darkBg : c.bg, highlightName: c.name };
  }
  if (role.id === 5) {
    const m = WORD_HIGHLIGHT_MAP.magenta;
    return { label: role.name, icon: m.icon, color: isDark ? m.darkColor : m.color, bg: isDark ? m.darkBg : m.bg, highlightName: m.name };
  }

  // 3. 若角色配置了专属墨水颜色
  if (role.fill) {
    return {
      label: role.name,
      icon: "🎨",
      color: role.fill,
      bg: isDark ? `${role.fill}44` : `${role.fill}1a`,
      highlightName: `墨水 ${role.fill}`,
    };
  }

  // 4. 其余角色的循环调色板
  const palette = [
    { name: "紫色", bg: "#f3e8ff", color: "#6b21a8", darkBg: "#581c87", darkColor: "#f3e8ff", icon: "🟣" },
    { name: "橙色", bg: "#ffedd5", color: "#9a3412", darkBg: "#7c2d12", darkColor: "#ffedd5", icon: "🟠" },
    { name: "粉红", bg: "#ffe4e6", color: "#9f1239", darkBg: "#881337", darkColor: "#ffe4e6", icon: "🌸" },
    { name: "青绿", bg: "#ccfbf1", color: "#115e59", darkBg: "#134e4a", darkColor: "#ccfbf1", icon: "🟢" },
    { name: "靛蓝", bg: "#e0e7ff", color: "#3730a3", darkBg: "#312e81", darkColor: "#e0e7ff", icon: "🔵" },
  ];
  const p = palette[(role.id - 6 + palette.length) % palette.length];
  return {
    label: role.name,
    icon: p.icon,
    color: isDark ? p.darkColor : p.color,
    bg: isDark ? p.darkBg : p.bg,
    highlightName: p.name,
  };
}

export interface Para {
  id: number;
  text: string;
  align: 0 | 1 | 2;
  indentEm: number;
  runs?: UiTextRun[];
}

export type Region = UiRegion;

export const defaultRoles = (): UiHandwritingRole[] => [
  { id: 0, name: "默认手写 (主字体)", fontPath: "", printed: false, fill: null },
  { id: 1, name: "打印体 (无扰动)", fontPath: "", printed: true, fill: null },
];

let paraSeq = 1;
export const newPara = (
  text = "",
  align: 0 | 1 | 2 = 0,
  indentEm = 0,
  runs?: UiTextRun[],
): Para => ({
  id: paraSeq++,
  text,
  align,
  indentEm,
  runs,
});

/** 清理外来文本特殊字符（对齐后端 clean_text / 原 to_ui_spaces） */
export const cleanText = (s: string): string =>
  s.replace(/\u2060/g, "").replace(/[\u00a0\uffa0]/g, " ");

const PREVIEW_BG_COLORS = ["#c8d0ca", "#565b56"];

/** 新建图形的默认样式：null = 自动（颜色跟随主墨色，线宽 / 抖动按纸张自适应）。 */
export interface ShapeDefaultStyle {
  /** "#RRGGBB"；null = 跟随主墨色 store.fontColor */
  fill: string | null;
  /** 线宽（背景原始像素）；null = 按纸张自适应 */
  thickness: number | null;
  /** 手绘抖动（背景原始像素）；null = 按线宽自适应 */
  jitter: number | null;
}

export const store = reactive({
  // ---- 系统字体缓存 ----
  systemFonts: [] as SystemFontItem[],

  // ---- 参数表单（与 Slint 版控件一一对应）----
  fontPath: "",
  backgroundPath: "",
  fontSize: 36,
  wordSpacing: 5,
  lineSpacing: 48,
  wordSpacingSigma: 2,
  lineSpacingSigma: 2,
  fontSizeSigma: 2,
  perturbX: 2,
  perturbY: 2,
  perturbThetaText: "0.05",
  miswriteRate: 0, // 百分比 0~30
  miswriteModeIndex: 0,
  strikeoutStyleIndex: 0,
  fontColor: "#000000",
  marginTop: 30,
  marginBottom: 30,
  marginLeft: 30,
  marginRight: 30,
  boundsVisible: false,
  boundsColor: "#4ca6a6",
  endChars: "，。",
  startChars: "",

  // ---- 笔迹角色 ----
  roles: defaultRoles(),

  // ---- 逐段编辑器 ----
  paragraphs: [newPara()] as Para[],
  curParaId: null as number | null,
  paraStatus: "光标定位到段落后可用按钮设置格式",
  /** 编辑器内存在非折叠文字选区（驱动右栏「选区角色」标记条按需显示） */
  hasTextSelection: false,

  // ---- 框选文字区域 ----
  regionMode: false,
  regions: [] as Region[],
  /** 当前内嵌编辑的区域索引（-1 = 无；预览上的调整框与右侧编辑卡片共用） */
  editingIndex: -1,
  highlightIndex: -1,
  selectedRegionIndex: -1,

  // ---- 图形标注（坐标与 regions 一致：背景原始像素）----
  /** 图形标注模式（与 regionMode 互斥） */
  shapeMode: false,
  /** 当前要画的图元，默认矩形 */
  shapeKind: "rect" as UiShapeKind,
  /** 全量图形（跨页；数组顺序 = 绘制顺序，后画的在上层） */
  shapes: [] as UiShape[],
  /** 图形列表/预览当前选中的索引（-1 = 未选中） */
  selectedShapeIndex: -1,
  /** 新建图形默认样式：只作用于之后新画的图形；已画图形各自的样式存在 shape 上 */
  shapeDefault: { fill: null, thickness: null, jitter: null } as ShapeDefaultStyle,

  // ---- 区域对话框（草稿模式：打开时拷贝，确定时写回）----
  pendingRect: null as [number, number, number, number] | null,
  dialogOpen: false,
  dialogTargetIndex: -1, // -1 = 新建
  dialogDraft: null as Region | null,

  docPages: null as string[] | null, // 文档底图逐页 PNG 路径
  docStatus: "",

  // ---- 预览 ----
  previewPages: [] as { url: string; width: number; height: number }[],
  pageIndex: 0,
  previewBgIdx: 0,
  /** 列表「🔍」请求聚焦的区域索引（-1 = 无）；token 自增驱动预览组件应用 */
  previewFocusIndex: -1,
  previewFocusToken: 0,
  bgNatW: 0,
  bgNatH: 0,
  rendering: false,
  status: "就绪",

  presets: [] as { name: string; path: string }[],

  // ---- 当前方案（预设 / 字体 / 纸张）----
  currentPresetPath: "",
  currentPresetName: "",
  presetDirty: false,
  /** 预设资产缺失等非阻断提示，显示在「当前方案」卡片里 */
  presetNotice: "",
  /** 自动兜底选中的系统字体路径（用于提示「已自动使用系统字体」） */
  autoFontPath: "",

  // ---- 新手引导 / 首屏状态 ----
  onboardingOpen: false,
  onboardingDone: readStoredBool(KEY_ONBOARDING_DONE),
  checklistDismissed: readStoredBool(KEY_CHECKLIST_DISMISSED),
  hasExported: readStoredBool(KEY_HAS_EXPORTED),
  textEditorOpen: false,

  // ---- 可恢复的错误（替代只在状态栏里报错）----
  errorBanner: "",
  errorKind: "" as "" | "font" | "background" | "other",
  errorSource: "" as "" | "render" | "export" | "preset",

  // ---- 关于与版本更新 ----
  aboutModalOpen: false,
  helpModalOpen: false,
  helpTab: "quickstart",
  updateModalOpen: false,
  updateInfo: null as UpdateInfo | null,
  checkingUpdate: false,
  updateStatusText: "点击右侧按钮可主动联网检测最新版本",
  autoCheckUpdate: isAutoCheckEnabled(),
  skippedVersion: getSkippedVersion(),

  // ---- 主题模式 ----
  themePreference: getStoredThemePreference(),
  systemIsDark: typeof window !== "undefined" && window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)").matches : false,
});

/** 当前是否实际生效深色模式 */
export function isDarkActive(): boolean {
  if (store.themePreference === "dark") return true;
  if (store.themePreference === "light") return false;
  return store.systemIsDark;
}

/** 设置主题偏好（'auto' | 'light' | 'dark'）并持久化 */
export function setThemePreference(pref: ThemePreference): void {
  store.themePreference = pref;
  try {
    localStorage.setItem(KEY_THEME_PREFERENCE, pref);
  } catch (e) {
    console.error("保存主题配置失败:", e);
  }
  syncThemeClass();
}

/** 循环切换主题偏好：自动跟随系统 ➔ 浅色模式 ➔ 深色模式 ➔ 自动跟随系统 */
export function cycleThemePreference(): void {
  if (store.themePreference === "auto") {
    setThemePreference("light");
  } else if (store.themePreference === "light") {
    setThemePreference("dark");
  } else {
    setThemePreference("auto");
  }
}

/** 兼容旧的 toggleTheme 调用 */
export const toggleTheme = cycleThemePreference;

/** 同步 html/body 的 dark 类与 data-theme 属性 */
export function syncThemeClass(): void {
  if (typeof document !== "undefined") {
    const dark = isDarkActive();
    if (dark) {
      document.documentElement.classList.add("dark");
      document.documentElement.setAttribute("data-theme", "dark");
    } else {
      document.documentElement.classList.remove("dark");
      document.documentElement.setAttribute("data-theme", "light");
    }
  }
}

/** 初始化主题系统（注册 matchMedia 与 Tauri 原生系统主题变化监听） */
export function initThemeSystem(): void {
  // 1. Web mediaQuery 监听（支持现代 addEventListener 与旧版 addListener）
  if (typeof window !== "undefined" && window.matchMedia) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    store.systemIsDark = mq.matches;

    const handler = (e: MediaQueryListEvent | { matches: boolean }) => {
      store.systemIsDark = e.matches;
      if (store.themePreference === "auto") {
        syncThemeClass();
      }
    };

    if (mq.addEventListener) {
      mq.addEventListener("change", handler);
    } else if ((mq as any).addListener) {
      (mq as any).addListener(handler);
    }
  }

  // 2. Tauri 原生 OS 主题变化监听（Windows / macOS / Linux）
  try {
    api.getSystemTheme()
      .then((theme) => {
        store.systemIsDark = theme === "dark";
        syncThemeClass();
      })
      .catch(() => {});

    api.onSystemThemeChanged((theme) => {
      store.systemIsDark = theme === "dark";
      if (store.themePreference === "auto") {
        syncThemeClass();
      }
    });
  } catch (e) {
    // 纯网页调试环境兼容
  }

  syncThemeClass();
}

// 模块加载时立即初始化一次
initThemeSystem();


// ---------------------------------------------------------------- 首次使用与当前方案

/** 取路径中的文件名（兼容 Windows 反斜杠）。 */
export function fileName(path: string): string {
  const s = path.replace(/\\/g, "/");
  const i = s.lastIndexOf("/");
  return i >= 0 ? s.slice(i + 1) : s;
}

/** 以目录原始分隔符拼接路径（Windows 用 \，Unix 用 /）。 */
function joinFsPath(dir: string, ...parts: string[]): string {
  const sep = dir.includes("\\") ? "\\" : "/";
  return [dir.replace(/[\\/]+$/, ""), ...parts].join(sep);
}

/** 当前字体的人类可读名称（系统字体用名称，本地字体用文件名）。 */
export function currentFontLabel(): string {
  const p = store.fontPath.trim();
  if (!p) return "未选择字体";
  const found = store.systemFonts.find((f) => f.path === p);
  return found ? found.name : fileName(p);
}

export function currentBackgroundLabel(): string {
  if ((store.docPages?.length ?? 0) > 0) return `文档底图 ${store.docPages!.length} 页`;
  const p = store.backgroundPath.trim();
  return p ? fileName(p) : "未选择纸张";
}

export function hasTextContent(): boolean {
  return store.paragraphs.some(
    (p) =>
      cleanText(p.text).trim() !== "" ||
      (p.runs?.some((run) => cleanText(run.text).trim() !== "") ?? false),
  );
}

export function hasContent(): boolean {
  return (
    hasTextContent() ||
    (store.docPages?.length ?? 0) > 0 ||
    store.regions.some((r) => r.text.trim() !== "") ||
    store.shapes.length > 0
  );
}

export type WorkflowMode = "empty" | "text" | "document";

/** 当前处于哪种工作流：空 / 纯文字 / 文档填空。 */
export function workflowMode(): WorkflowMode {
  if ((store.docPages?.length ?? 0) > 0) return "document";
  return hasTextContent() || store.regions.length > 0 || store.shapes.length > 0 ? "text" : "empty";
}

/** 首启期间 App 跳过了更新检查；引导结束后补一次，避免新用户整场都不检查更新。 */
function scheduleStartupUpdateCheck(): void {
  if (!store.autoCheckUpdate) return;
  window.setTimeout(() => {
    if (!store.onboardingOpen && !store.checkingUpdate) void startupCheckUpdate();
  }, 3000);
}

export function finishOnboarding(): void {
  store.onboardingDone = true;
  writeStoredBool(KEY_ONBOARDING_DONE, true);
  store.onboardingOpen = false;
  scheduleStartupUpdateCheck();
}

export function openOnboarding(): void {
  store.onboardingOpen = true;
}

/** 首次启动且没看过引导时自动弹出（用户已导入过内容则不打扰）。 */
export function maybeOpenOnboarding(): void {
  if (!store.onboardingDone && !hasContent()) store.onboardingOpen = true;
}

/** 关闭引导（跳过 / 点了关闭都算已看过，不再自动弹出）。 */
export function dismissOnboarding(): void {
  store.onboardingOpen = false;
  store.onboardingDone = true;
  writeStoredBool(KEY_ONBOARDING_DONE, true);
  scheduleStartupUpdateCheck();
}

export function dismissChecklist(): void {
  store.checklistDismissed = true;
  writeStoredBool(KEY_CHECKLIST_DISMISSED, true);
}

export function markExported(): void {
  store.hasExported = true;
  writeStoredBool(KEY_HAS_EXPORTED, true);
}

/**
 * 字体兜底：用户没选字体时，自动挑一款系统字体（楷体优先）。
 * 这样首次启动不会因为「未指定字体文件」直接失败。
 */
export async function ensureUsableFont(): Promise<string> {
  const existing = store.fontPath.trim();
  if (existing) {
    // 路径可能来自旧预设 / 已被用户删除：失效时清掉再兜底
    try {
      if (await api.pathExists(existing)) return existing;
    } catch {
      // path_exists 不可用时按可用处理，交给渲染错误横幅兜底
      return existing;
    }
    store.fontPath = "";
    if (store.autoFontPath === existing) store.autoFontPath = "";
  }
  if (store.systemFonts.length === 0) await initSystemFonts();

  const candidates = ["楷体", "华文楷体", "华文行楷", "楷体_GB2312", "仿宋", "苹方", "微软雅黑", "宋体"];
  for (const name of candidates) {
    const f = matchSystemFont(name);
    if (f) {
      store.fontPath = f.path;
      store.autoFontPath = f.path;
      return f.path;
    }
  }

  const first =
    store.systemFonts.find((f) =>
      /kai|song|hei|yahei|noto.*cjk|sourcehan|cjk|pingfang|苹方|hiragino|stheiti|songti/i.test(
        `${f.name} ${f.family} ${f.path}`,
      ),
    ) ?? store.systemFonts[0];
  if (first) {
    store.fontPath = first.path;
    store.autoFontPath = first.path;
    return first.path;
  }
  return "";
}

/**
 * 纸张兜底：没选背景时，从内置预设里借一张确实存在的纸张，
 * 避免「字体好了、背景又是空」的二次失败。
 */
export async function ensureUsableBackground(ignoreCurrent = false): Promise<string> {
  const existing = store.backgroundPath.trim();
  if (!ignoreCurrent && existing) {
    try {
      if (await api.pathExists(existing)) return existing;
    } catch {
      return existing;
    }
    store.backgroundPath = "";
  }
  if (store.presets.length === 0) await refreshPresets();

  // 首屏优先纯白 / A4 白纸：版式参数（140px/97 行距）按 A4 纸张给，白纸最稳。
  // 其次才是任意 A4、任意白纸；都没有时退回任意内置纸张。
  const score = (name: string) => {
    const n = name.toLowerCase();
    if (n.includes("a4") && n.includes("白纸")) return 3;
    if (n.includes("白纸")) return 2;
    if (n.includes("a4")) return 1;
    return 0;
  };
  const ordered = [...store.presets].sort((a, b) => score(b.name) - score(a.name));
  const preferred = ordered.filter((p) => score(p.name) > 0);
  const candidates = preferred.length > 0 ? preferred : ordered;

  for (const p of candidates) {
    try {
      const params = await api.loadPreset(p.path);
      const bg = (params.backgroundPath || "").trim();
      if (bg && (await api.pathExists(bg))) {
        store.backgroundPath = bg;
        return bg;
      }
    } catch {
      // 单个预设损坏不影响兜底
    }
  }

  // presets/ 缺失或全部不可读时，直接探测 exe 旁的 backgrounds/：
  // 发布包/开发仓库都带有这张 A4 纯白，不能因为预设坏了就没有纸。
  try {
    const presetDir = await api.defaultPresetDir();
    const root = presetDir.replace(/[\\/]presets[\\/]?$/i, "");
    for (const name of ["A4纯白.webp", "实线信纸.webp"]) {
      const candidate = joinFsPath(root, "backgrounds", name);
      if (await api.pathExists(candidate)) {
        store.backgroundPath = candidate;
        return candidate;
      }
    }
  } catch {
    // 命令层不可用时保持原有兜底结果
  }
  return ignoreCurrent ? "" : store.backgroundPath;
}

/**
 * 首屏默认版式：内置纸张是 4200×5930 级的 A4 图，
 * 直接沿用 36px 字号会小到看不清。这里按 A4 纸张给一套和内置预设同量级的
 * 起步参数（只在用户还没动过参数、也没载入预设时生效）。
 */
export async function ensureStarterLayout(): Promise<void> {
  if (store.currentPresetPath) return;
  if (hasContent()) return;
  const atDefaults =
    store.fontSize === 36 &&
    store.wordSpacing === 5 &&
    store.lineSpacing === 48 &&
    store.marginTop === 30 &&
    store.marginBottom === 30 &&
    store.marginLeft === 30 &&
    store.marginRight === 30;
  if (!atDefaults) return;
  if (!store.backgroundPath.trim()) await ensureUsableBackground();

  store.fontSize = 140;
  store.wordSpacing = 0;
  store.lineSpacing = 97;
  store.wordSpacingSigma = 2;
  store.lineSpacingSigma = 4;
  store.fontSizeSigma = 4;
  store.perturbX = 5;
  store.perturbY = 6;
  store.perturbThetaText = "0.07";
  store.marginTop = 300;
  store.marginBottom = 600;
  store.marginLeft = 350;
  store.marginRight = 350;
  store.fontColor = "#130e0b";
}

/** 打开正文编辑弹窗（先保证有可用字体，避免编辑完渲染失败）。 */
export async function openTextEditor(): Promise<void> {
  await ensureUsableFont();
  store.textEditorOpen = true;
}

export function closeTextEditor(): void {
  store.textEditorOpen = false;
}

export const SAMPLE_PARAGRAPHS = [
  "见字如面。",
  "最近天气转凉，夜里写作业时总觉得手有些僵。我把这段时间的笔记重新誊了一遍，字还是有点潦草，你别见笑。",
  "等忙完这一阵，找个周末回去看看。到时候一起去巷口那家小馆子，吃顿热乎的。",
];

/** 一键示例：铺好字体、纸张、版式和正文，直接出第一张成品。 */
export async function loadSample(): Promise<void> {
  await ensureUsableFont();
  await ensureUsableBackground();

  store.currentPresetPath = "";
  store.currentPresetName = "";
  store.presetDirty = false;
  store.presetNotice = "";

  store.fontSize = 140;
  store.wordSpacing = 0;
  store.lineSpacing = 97;
  store.wordSpacingSigma = 2;
  store.lineSpacingSigma = 4;
  store.fontSizeSigma = 4;
  store.perturbX = 5;
  store.perturbY = 6;
  store.perturbThetaText = "0.07";
  store.marginTop = 300;
  store.marginBottom = 600;
  store.marginLeft = 350;
  store.marginRight = 350;
  store.fontColor = "#130e0b";
  store.miswriteRate = 0;
  store.boundsVisible = false;
  store.roles = defaultRoles();
  store.regions.splice(0);
  store.regionMode = false;
  store.editingIndex = -1;
  store.highlightIndex = -1;
  store.selectedRegionIndex = -1;
  // 示例会换纸张：旧纸上的图形坐标随之失效，一并清空并退出标注模式
  store.shapes.splice(0);
  store.shapeMode = false;
  store.selectedShapeIndex = -1;
  store.docPages = null;
  store.docStatus = "";
  setParagraphs(SAMPLE_PARAGRAPHS.map((text, i) => newPara(text, 0, i === 0 ? 0 : 2)));

  store.currentPresetPath = "";
  store.currentPresetName = "";
  store.presetDirty = false;
  store.status = "已载入示例，可直接导出，也可以换成你自己的文字";

  finishOnboarding();
  await doRender();
  store.status = "已载入示例：可直接导出，或换成你自己的文字";
}

/** 首次引导的三个入口：写文字 / 填文档 / 看示例。 */
export async function handleOnboardingTask(task: "text" | "document" | "sample"): Promise<void> {
  // 只有「示例」需要在这里预确认；「填文档」由 importDocument 自己确认一次，避免连弹两次
  if (task === "sample" && hasContent()) {
    const ok = await confirmReplaceContent();
    if (!ok) return;
  }

  if (task === "text") {
    // 文档模式不显示主文字：先退出文档/区域模式，再打开正文编辑器
    if (!(await confirmDiscardDocPages("text"))) return;
    if ((store.docPages?.length ?? 0) > 0 || store.regions.length > 0) {
      store.docPages = null;
      store.docStatus = "";
      store.regions.splice(0);
      store.regionMode = false;
      store.editingIndex = -1;
      store.highlightIndex = -1;
      store.selectedRegionIndex = -1;
      // 退出文档模式会换回普通纸张，图形坐标同样失效
      store.shapes.splice(0);
      store.shapeMode = false;
      store.selectedShapeIndex = -1;
      store.backgroundPath = "";
      await ensureUsableBackground();
    }
    await ensureUsableFont();
    await ensureUsableBackground();
    await ensureStarterLayout();
    finishOnboarding();
    await openTextEditor();
    scheduleRender();
    return;
  }

  await ensureUsableFont();
  await ensureUsableBackground();
  await ensureStarterLayout();
  finishOnboarding();

  if (task === "document") {
    await importDocument();
    return;
  }
  await loadSample();
}




// ---------------------------------------------------------------- 参数构建

/** 有效文档底图页：背景被手动改走后自动失效（对齐 Python 版 _sync_doc_state） */
function validDocPages(): string[] {
  const pages = store.docPages;
  return pages && pages[0] === store.backgroundPath.trim() ? [...pages] : [];
}

const num = (v: number | null | undefined, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? v : fallback;

/** 收集表单为引擎参数（对齐原 collect_params 的段落/纯文本分支语义）。 */
export function buildParams(): UiParams {
  const paras = store.paragraphs
    .filter((r) => cleanText(r.text).trim() !== "" || (r.runs && r.runs.some((run) => cleanText(run.text).trim() !== "")))
    .map((r) => ({
      text: cleanText(r.text),
      align: r.align,
      indentEm: r.indentEm,
      runs: r.runs?.map((run) => ({
        text: cleanText(run.text),
        style: run.style ? { ...run.style } : undefined,
      })),
    }));
  const hasFormat = store.paragraphs.some(
    (r) => r.align !== 0 || r.indentEm !== 0 || (r.runs && r.runs.length > 0),
  );

  let text = "";
  let outParas = paras;
  if (!(paras.length > 1 || hasFormat)) {
    text = cleanText(paras[0]?.text ?? "").trim();
    outParas = [];
  }

  const theta = parseFloat(store.perturbThetaText.trim());
  return {
    fontPath: store.fontPath.trim(),
    backgroundPath: store.backgroundPath.trim(),
    backgroundPages: validDocPages(),
    fontSize: num(store.fontSize, 36),
    wordSpacing: num(store.wordSpacing, 5),
    lineSpacing: num(store.lineSpacing, 48),
    wordSpacingSigma: num(store.wordSpacingSigma, 2),
    lineSpacingSigma: num(store.lineSpacingSigma, 2),
    fontSizeSigma: num(store.fontSizeSigma, 2),
    perturbXSigma: num(store.perturbX, 2),
    perturbYSigma: num(store.perturbY, 2),
    perturbThetaSigma: Number.isFinite(theta) ? theta : 0.05,
    marginTop: num(store.marginTop, 30),
    marginBottom: num(store.marginBottom, 30),
    marginLeft: num(store.marginLeft, 30),
    marginRight: num(store.marginRight, 30),
    fill: store.fontColor,
    miswriteRate: num(store.miswriteRate, 0) / 100,
    miswriteModeIndex: store.miswriteModeIndex,
    miswriteStrikeoutStyleIndex: store.strikeoutStyleIndex,
    text,
    paragraphs: outParas,
    regions: store.regions.map((r) => ({ ...r })),
    // 深拷贝 points：不能把响应式引用直接递给 IPC（序列化时会读到 Vue proxy）
    shapes: store.shapes.map((s) => ({
      ...s,
      points: s.points?.map((p): [number, number] => [p[0], p[1]]),
    })),
    roles: store.roles.map((r) => ({ ...r })),
    endChars: store.endChars,
    startChars: store.startChars,
    boundsVisible: store.boundsVisible,
    boundsColor: store.boundsColor,
  };
}

// ---------------------------------------------------------------- 预览渲染

let renderSeq = 0;

function classifyRenderError(message: string): "font" | "background" | "other" {
  // 角色 / 文字区域的字体、文档后续页背景都不是「换主字体 / 换纸张」能修的
  if ((message.includes("角色") || message.includes("文字区域")) && message.includes("字体")) {
    return "other";
  }
  if (message.includes("页背景")) return "other";
  if (message.includes("字体")) return "font";
  if (message.includes("背景")) return "background";
  return "other";
}

/** 立即重渲染（后台执行，UI 不阻塞；过期结果丢弃）。 */
export async function doRender(): Promise<void> {
  const seq = ++renderSeq;
  store.rendering = true;
  store.status = "渲染中…";
  try {
    const pages = await api.renderPreview(buildParams());
    if (seq !== renderSeq) return;
    const stamp = Date.now();
    store.previewPages = pages.map((p) => ({
      url: `${assetUrl(p.path)}?v=${stamp}`,
      width: p.width,
      height: p.height,
    }));
    if (store.pageIndex >= pages.length) store.pageIndex = 0;
    store.status = `预览完成，共 ${pages.length} 页`;
    // 只清理「渲染产生」的错误：导出/预设错误不应被一次成功渲染顺手擦掉
    if (store.errorSource === "render") {
      store.errorBanner = "";
      store.errorKind = "";
      store.errorSource = "";
    }
  } catch (e) {
    if (seq !== renderSeq) return;
    const message = String(e);
    store.status = `渲染失败：${message}`;
    store.errorBanner = message;
    store.errorKind = classifyRenderError(message);
    store.errorSource = "render";
  } finally {
    if (seq === renderSeq) store.rendering = false;
  }
}

const debouncedRender = useDebounceFn(doRender, 300);
export const scheduleRender = (): void => void debouncedRender();
export const triggerPreview = scheduleRender;

export function prevPage(): void {
  if (store.pageIndex > 0) {
    store.pageIndex -= 1;
    // 图形按页归属：切页后清选中，避免属性面板改到别的页上的图形
    store.selectedShapeIndex = -1;
  }
}

export function nextPage(): void {
  if (store.pageIndex + 1 < store.previewPages.length) {
    store.pageIndex += 1;
    store.selectedShapeIndex = -1;
  }
}

export function togglePreviewBg(): void {
  store.previewBgIdx = (store.previewBgIdx + 1) % PREVIEW_BG_COLORS.length;
}

// ---------------------------------------------------------------- 背景尺寸与文档底图

/** 加载背景自然尺寸（供框选坐标换算）。
 *
 *  走后端 image_dimensions 只读图片头——不依赖 asset 协议，
 *  任意磁盘路径的背景都能拿到尺寸（asset scope 仅覆盖预览缓存目录）。 */
export async function loadBgDimensions(path: string): Promise<void> {
  if (!path) {
    store.bgNatW = 0;
    store.bgNatH = 0;
    return;
  }
  try {
    const dims = await api.imageDimensions(path);
    store.bgNatW = dims?.[0] ?? 0;
    store.bgNatH = dims?.[1] ?? 0;
  } catch {
    store.bgNatW = 0;
    store.bgNatH = 0;
  }
}

/** 手动改走背景路径时使文档底图失效。 */
function syncDocState(): void {
  const dp = store.docPages;
  if (dp && dp[0] !== store.backgroundPath.trim()) {
    store.docPages = null;
    store.docStatus = "";
  }
}

// 背景路径变化：刷新自然尺寸（供框选坐标换算）+ 文档底图同步
watch(
  () => store.backgroundPath,
  (p) => {
    loadBgDimensions(p);
    syncDocState();
  },
);

// ---------------------------------------------------------------- 系统字体管理与匹配

let systemFontsPromise: Promise<void> | null = null;

export async function initSystemFonts(): Promise<void> {
  if (store.systemFonts.length > 0) return;
  if (!systemFontsPromise) {
    systemFontsPromise = (async () => {
      try {
        store.systemFonts = await api.listSystemFonts();
      } catch (e) {
        console.warn("获取系统字体列表失败:", e);
        store.systemFonts = [];
        // 允许下次重试（例如启动早期命令层尚未就绪）
        systemFontsPromise = null;
      }
    })();
  }
  await systemFontsPromise;
}

const FONT_ALIAS_GROUPS: string[][] = [
  ["仿宋", "fangsong", "仿宋_gb2312", "fangsong_gb2312", "simfang", "stfangsong", "华文仿宋"],
  ["楷体", "kaiti", "楷体_gb2312", "kaiti_gb2312", "simkai", "stkaiti", "kaitisc", "华文楷体"],
  ["微软雅黑", "microsoftyahei", "microsoftyaheiui", "yahei", "msyh"],
  ["宋体", "simsun", "新宋体", "nsimsun", "songtisc", "songti", "stsong", "华文宋体"],
  ["黑体", "simhei", "heitisc", "heiti", "stxihei", "stheitilight", "stheitimedium", "华文细黑", "华文黑体"],
  ["等线", "dengxian", "deng"],
  ["幼圆", "youyuan", "simyou"],
  ["隶书", "lisu", "simli", "stliti", "华文隶书"],
  ["华文行楷", "stxingkai", "xingkai"],
  ["华文中宋", "stzhongsong"],
  ["华文琥珀", "sthupo"],
  ["华文彩云", "stcaiyun"],
  ["华文新魏", "stxinwei"],
  ["方正舒体", "fzshuti", "fzstk"],
  ["方正姚体", "fzyaoti", "fzytk"],
  ["苹方", "pingfang", "pingfangsc"],
  ["思源黑体", "notosanssc", "notosanscjk", "notosans", "sourcehansanscn", "sourcehansans"],
  ["思源宋体", "notoserifsc", "notoserifcjk", "notoserif", "sourcehanserifcn", "sourcehanserif"],
  ["文泉驿微米黑", "wenquanyimicrohei", "wqymicrohei"],
  ["文泉驿正黑", "wenquanyizenhei", "wqyzenhei"],
  ["arial", "arialmt"],
  ["timesnewroman", "times", "timeroman"],
  ["calibri"],
  ["couriernew", "courier"],
  ["tahoma"],
  ["verdana"],
  ["segoeui", "segoe"],
];

function normalizeFontStr(s: string): string {
  return s.toLowerCase().replace(/[\s\-_]/g, "").replace(/gb2312|regular|normal|bold|italic|ui|sc|light/g, "");
}

/** 智能模糊匹配系统字体（支持中英文别名、简写及规范化匹配） */
export function matchSystemFont(fontName?: string | null): SystemFontItem | undefined {
  if (!fontName || !fontName.trim() || store.systemFonts.length === 0) return undefined;
  const raw = fontName.trim();
  const rawLower = raw.toLowerCase();
  const rawNorm = rawLower.replace(/[\s\-_]/g, "");

  // 1. 尝试完全/规范化精确匹配（name, family, 或 path 中文件名）
  let found = store.systemFonts.find(
    (f) =>
      f.name.toLowerCase() === rawLower ||
      f.family.toLowerCase() === rawLower ||
      f.name.toLowerCase().replace(/[\s\-_]/g, "") === rawNorm ||
      f.family.toLowerCase().replace(/[\s\-_]/g, "") === rawNorm,
  );
  if (found) return found;

  // 2. 尝试别名组匹配
  for (const group of FONT_ALIAS_GROUPS) {
    const matchesInput = group.some(
      (alias) =>
        rawNorm === alias ||
        rawNorm.includes(alias) ||
        alias.includes(rawNorm) ||
        normalizeFontStr(raw) === normalizeFontStr(alias),
    );
    if (matchesInput) {
      for (const alias of group) {
        found = store.systemFonts.find((f) => {
          const fn = f.name.toLowerCase().replace(/[\s\-_]/g, "");
          const ff = f.family.toLowerCase().replace(/[\s\-_]/g, "");
          const fp = f.path.toLowerCase().replace(/[\s\-_]/g, "");
          return fn.includes(alias) || ff.includes(alias) || fp.includes(alias);
        });
        if (found) return found;
      }
    }
  }

  // 3. 包含关系模糊匹配
  const stripped = normalizeFontStr(raw);
  if (stripped.length >= 2) {
    found = store.systemFonts.find((f) => {
      const fn = normalizeFontStr(f.name);
      const ff = normalizeFontStr(f.family);
      return (
        fn.includes(stripped) ||
        ff.includes(stripped) ||
        stripped.includes(fn) ||
        stripped.includes(ff)
      );
    });
    if (found) return found;
  }

  return undefined;
}

// ---------------------------------------------------------------- 文件选择类动作

export async function chooseFont(): Promise<void> {
  const p = await dialogs.pickFont();
  if (typeof p === "string") store.fontPath = p;
}

export async function chooseBackground(): Promise<void> {
  // 换纸会改背景原始尺寸，而图形/区域坐标是绝对像素：旧标注会错位或跑出可见区。
  // 这里先确认再换（与切预设的语义对齐），并把图形清掉（区域维持既有行为，不动）。
  if (store.shapes.length > 0) {
    const ok = await new Promise<boolean>((resolve) => {
      let settled = false;
      const finish = (v: boolean) => {
        if (settled) return;
        settled = true;
        resolve(v);
      };
      appDialog.warning({
        title: "换纸会清除图形标注",
        content: `当前有 ${store.shapes.length} 个图形标注，它们记录的是这张纸上的像素坐标；换纸后位置会错位。继续会清空图形标注。`,
        positiveText: "继续换纸",
        negativeText: "取消",
        onPositiveClick: () => finish(true),
        onNegativeClick: () => finish(false),
        onClose: () => finish(false),
        onMaskClick: () => finish(false),
      });
    });
    if (!ok) return;
    resetShapes();
  }
  const p = await dialogs.pickImage();
  if (typeof p === "string") store.backgroundPath = p;
}

/**
 * 当导入的 Word 文档包含高亮或背景色标记时，询问用户想要的效果：
 * - 'mixed': 混合填空模式（高亮文字设为手写体，未标记文字转为打印体模板）
 * - 'all_handwriting': 全文手写模式（忽略高亮标记，所有文字统一作为手写体排版）
 * - 'cancel': 取消导入
 */
export function askDocxHighlightMode(): Promise<"mixed" | "all_handwriting" | "cancel"> {
  return new Promise((resolve) => {
    let resolved = false;
    appDialog.create({
      title: "Word 文档高亮排版模式",
      content:
        "检测到该 Word 文档中包含背景高亮或颜色标记，请选择排版方式：\n\n" +
        "• 混合填空模式：高亮文字识别为手写体，未标记文字保留为打印体（适合试卷/表格填空）。\n\n" +
        "• 全文手写模式：忽略所有高亮背景标记，文档全部内容统一作为手写体排版（适合保留原模板、全篇手写）。",
      positiveText: "混合填空模式",
      negativeText: "全文手写模式",
      closable: true,
      maskClosable: true,
      onPositiveClick: () => {
        resolved = true;
        resolve("mixed");
      },
      onNegativeClick: () => {
        resolved = true;
        resolve("all_handwriting");
      },
      onClose: () => {
        if (!resolved) {
          resolved = true;
          resolve("cancel");
        }
      },
      onMaskClick: () => {
        if (!resolved) {
          resolved = true;
          resolve("cancel");
        }
      },
    });
  });
}

/**
 * 询问用户导入文档底图的处理方式：
 * - 'detect': 识别高亮填空（自动检测背景高亮色块并无痕擦除，提取标记文字为手写填空区域）
 * - 'pure': 仅作为纯底图（保留原始所有背景颜色与文字，不擦除底色、不自动生成选框）
 * - 'cancel': 取消导入
 */
export function askDocumentImportMode(): Promise<"detect" | "pure" | "cancel"> {
  return new Promise((resolve) => {
    let resolved = false;
    appDialog.create({
      title: "文档底图导入模式",
      content:
        "检测到您正在导入文档底图（PDF / Word），请选择导入效果：\n\n" +
        "• 识别高亮填空：自动检测并擦除背景高亮色块，将标记文字提取为手写填空区域（未标记文字保留为打印底图）。\n\n" +
        "• 仅作为纯底图：完整保留原文档所有背景色彩与版式，不擦除任何高亮，不自动生成选框（原样作为背景信纸）。",
      positiveText: "识别高亮填空",
      negativeText: "仅作为纯底图",
      closable: true,
      maskClosable: true,
      onPositiveClick: () => {
        resolved = true;
        resolve("detect");
      },
      onNegativeClick: () => {
        resolved = true;
        resolve("pure");
      },
      onClose: () => {
        if (!resolved) {
          resolved = true;
          resolve("cancel");
        }
      },
      onMaskClick: () => {
        if (!resolved) {
          resolved = true;
          resolve("cancel");
        }
      },
    });
  });
}

export async function importDocx(): Promise<void> {
  const p = await dialogs.pickDocx();
  if (!p) return;
  await ensureUsableFont();

  // Word 正文会覆盖当前内容：文档/区域/图形/已输入文字任一存在时都先确认
  const hadDoc = (store.docPages?.length ?? 0) > 0;
  const hadRegions = store.regions.length > 0;
  const hadShapes = store.shapes.length > 0;
  const hadText = hasTextContent();
  if (hadDoc || hadRegions || hadShapes || hadText) {
    const ok = await confirmReplaceContent();
    if (!ok) return;
  }

  try {
    if (store.systemFonts.length === 0) {
      await initSystemFonts();
    }
    const res = await api.importDocx(p, num(store.fontSize, 36));
    const rows = res.paragraphs;

    // 检查文档是否包含高亮或独立角色标记（混合模式 vs 纯手写模式）
    const hasHighlightsInDoc = rows.some((row) =>
      row.runs?.some(
        (r) =>
          r.style?.highlight != null ||
          ((r.style?.roleId ?? 0) >= 2 && !r.style?.printed),
      ),
    );

    let useHighlights = false;
    if (hasHighlightsInDoc) {
      const mode = await askDocxHighlightMode();
      if (mode === "cancel") return;
      useHighlights = mode === "mixed";
    }

    setParagraphs(
      rows.map((row) =>
        newPara(
          cleanText(row.text),
          (row.align ?? 0) as 0 | 1 | 2,
          row.indentEm ?? 0,
          row.runs?.map((r) => {
            const runStyle = r.style ? { ...r.style } : undefined;
            if (useHighlights) {
              // 混合模式：未高亮片段 (role_id == 1 或 printed == true)
              if (runStyle && (runStyle.roleId === 1 || runStyle.printed)) {
                if (runStyle.fontFamily) {
                  const matched = matchSystemFont(runStyle.fontFamily);
                  if (matched) {
                    runStyle.fontPath = matched.path;
                  }
                }
              }
            } else {
              // 纯手写模式：所有 run 清理为 role_id = 0, printed = false, fontPath = undefined, highlight = undefined
              if (runStyle) {
                runStyle.roleId = 0;
                runStyle.printed = false;
                runStyle.fontPath = undefined;
                runStyle.highlight = undefined;
              }
            }
            return {
              text: cleanText(r.text),
              style: runStyle,
            };
          }),
        ),
      ),
    );

    // 基础角色：Role 0 (主字体) 与 Role 1 (打印体)
    const role0: UiHandwritingRole = store.roles.find((r) => r.id === 0) || {
      id: 0,
      name: "默认手写 (主字体)",
      fontPath: "",
      printed: false,
      fill: null,
    };
    const role1: UiHandwritingRole = store.roles.find((r) => r.id === 1) || {
      id: 1,
      name: "打印体 (无扰动)",
      fontPath: "",
      printed: true,
      fill: null,
    };

    if (useHighlights) {
      // 自动匹配文档主字体 -> Role 1 (打印体)
      if (res.docFontFamily) {
        const matched = matchSystemFont(res.docFontFamily);
        if (matched) {
          role1.fontPath = matched.path;
        }
      }
    }

    const nextRoles: UiHandwritingRole[] = [role0, role1];

    if (useHighlights) {
      // 动态提取并自适应角色（收集 roleId >= 2 及对应的 highlight/fill）
      const detectedRoles = new Map<number, { highlight?: string | null; fill?: string | null }>();
      for (const row of rows) {
        if (row.runs) {
          for (const run of row.runs) {
            const rId = run.style?.roleId;
            if (rId && rId >= 2) {
              if (!detectedRoles.has(rId)) {
                detectedRoles.set(rId, {
                  highlight: run.style?.highlight ?? null,
                  fill: run.style?.fill ?? null,
                });
              } else {
                const cur = detectedRoles.get(rId)!;
                if (!cur.highlight && run.style?.highlight) cur.highlight = run.style.highlight;
                if (!cur.fill && run.style?.fill) cur.fill = run.style.fill;
              }
            }
          }
        }
      }

      if (detectedRoles.size > 0) {
        for (const [rId, info] of detectedRoles.entries()) {
          const hlInfo = getHighlightInfo(info.highlight);
          const highlightLabel = hlInfo?.name;
          const colorLabel = info.fill ? `颜色 ${info.fill}` : null;
          const subLabel = highlightLabel || colorLabel || "自定颜色";
          const roleName = `手写角色 ${rId - 1} (${subLabel})`;

          const prevRole = store.roles.find((r) => r.id === rId);

          nextRoles.push({
            id: rId,
            name: roleName,
            fontPath: prevRole?.fontPath || "",
            printed: false,
            highlight: info.highlight ?? null,
            fill: info.fill ?? null,
          });
        }
        nextRoles.sort((a, b) => a.id - b.id);
      }
    }

    // 保留未被本次导入覆盖的自定义角色：段落 runs 与既有 regions 仍可能引用它们，
    // 整表替换会让这些引用静默失效（对齐 importDocument 的合并行为）
    for (const existing of store.roles) {
      if (existing.id >= 2 && !nextRoles.some((r) => r.id === existing.id)) {
        nextRoles.push({ ...existing });
      }
    }
    nextRoles.sort((a, b) => a.id - b.id);
    store.roles = nextRoles;

    // 导入成功后再退出文档模式，失败时原文档底图/区域仍保留
    if (hadDoc) {
      store.docPages = null;
      store.docStatus = "";
      store.backgroundPath = "";
      await ensureUsableBackground();
    }
    if (hadRegions || hadShapes) {
      store.regions.splice(0);
      store.editingIndex = -1;
      store.highlightIndex = -1;
      store.selectedRegionIndex = -1;
      // 导入的是新版式：旧纸上的图形坐标失效，一并清空并退出标注模式
      store.shapes.splice(0);
      store.shapeMode = false;
      store.selectedShapeIndex = -1;
    }

    focusPara(store.paragraphs[0].id, 0);
    store.status = `已导入 ${rows.length} 个段落${useHighlights ? "（混合填空模式）" : "（全文手写模式）"}`;
    scheduleRender();
  } catch (e) {
    store.status = `导入 docx 失败：${e}`;
  }
}

export async function importDocument(): Promise<void> {
  const p = await dialogs.pickDocument();
  if (!p) return;
  await ensureUsableFont();

  // 文档模式不显示主文字：已有正文/底图/区域时先确认，导入成功后收起旧内容
  const hadText = hasTextContent();
  const hadDocState =
    (store.docPages?.length ?? 0) > 0 || store.regions.length > 0 || store.shapes.length > 0;
  if (hadText || hadDocState) {
    const ok = await confirmReplaceContent();
    if (!ok) return;
  }

  const mode = await askDocumentImportMode();
  if (mode === "cancel") return;

  const extractRegions = mode === "detect";
  store.status = extractRegions ? "正在智能识别文档高亮并渲染底图…" : "正在渲染原版文档底图…";
  try {
    const res = await api.importDocument(p, extractRegions);
    if (!res.pages.length) throw new Error("未得到任何页面");
    // 新文档底图会替换旧底图：旧区域坐标全部失效，先清空再写入本次识别结果
    store.regions.splice(0);
    store.editingIndex = -1;
    store.highlightIndex = -1;
    store.selectedRegionIndex = -1;
    // 图形与区域共用同一套背景坐标，换底图后一并清空并退出标注模式
    store.shapes.splice(0);
    store.shapeMode = false;
    store.selectedShapeIndex = -1;
    store.docPages = res.pages; // 先写 docPages 再切背景，避免 syncDocState 误清
    store.backgroundPath = res.pages[0];
    loadBgDimensions(res.pages[0]);
    if (extractRegions && res.regions.length > 0) {
      const detectedRoleIds = Array.from(
        new Set(
          res.regions
            .map((r) => r.roleId)
            .filter((id): id is number => typeof id === "number" && id >= 2),
        ),
      ).sort((a, b) => a - b);

      for (const roleId of detectedRoleIds) {
        const matchingRegion =
          res.regions.find((r) => r.roleId === roleId && r.highlight) ||
          res.regions.find((r) => r.roleId === roleId);
        const highlight = matchingRegion?.highlight ?? null;
        const hlInfo = getHighlightInfo(highlight);
        const name = `手写角色 ${roleId - 1}${hlInfo ? ` (${hlInfo.name})` : ""}`;
        const existingRole = store.roles.find((r) => r.id === roleId);
        if (existingRole) {
          if (!existingRole.highlight && highlight) existingRole.highlight = highlight;
        } else {
          store.roles.push({
            id: roleId,
            name,
            fontPath: "",
            printed: false,
            highlight: highlight ?? null,
            fill: null,
          });
        }
      }
      store.roles.sort((a, b) => a.id - b.id);

      store.regions = res.regions;
      store.selectedRegionIndex = 0;
      store.docStatus = `已导入 ${res.pages.length} 页，识别 ${res.regions.length} 处手写填空`;
      store.status = `已导入文档底图（共 ${res.pages.length} 页），并自动识别提取了 ${res.regions.length} 处手写填空区域！`;
    } else {
      store.docStatus = `已导入 ${res.pages.length} 页${extractRegions ? "" : "（纯底图模式）"}`;
      store.status = extractRegions
        ? `已导入文档底图（共 ${res.pages.length} 页，未检测到高亮填空标记）`
        : `已导入文档底图（共 ${res.pages.length} 页，已保留原文档全部背景与色彩）`;
    }
    // 成功切换后再收起正文：失败时用户的原文字还在
    if (hadText) setParagraphs([newPara()]);
    triggerPreview();
  } catch (e) {
    store.docStatus = "";
    store.status = `导入文档失败：${e}`;
    // DOCX 转 PDF 需要本机 Word/LibreOffice 之类，用户必须看到这个提示
    appDialog.error({
      title: "导入文档失败",
      content: String(e),
      positiveText: "知道了",
    });
  }
}

// ---------------------------------------------------------------- 预设

export function applyPreset(
  p: UiParams,
  msg?: string,
  meta?: { path?: string; name?: string },
): void {
  suppressPresetDirty = true;
  store.presetNotice = "";
  store.fontPath = p.fontPath;
  store.backgroundPath = p.backgroundPath;
  loadBgDimensions(p.backgroundPath);
  store.fontSize = Math.round(num(p.fontSize, 36));
  store.wordSpacing = Math.round(num(p.wordSpacing, 5));
  store.lineSpacing = Math.round(num(p.lineSpacing, 48));
  store.wordSpacingSigma = Math.round(num(p.wordSpacingSigma, 2));
  store.lineSpacingSigma = Math.round(num(p.lineSpacingSigma, 2));
  store.fontSizeSigma = Math.round(num(p.fontSizeSigma, 2));
  store.perturbX = Math.round(num(p.perturbXSigma, 2));
  store.perturbY = Math.round(num(p.perturbYSigma, 2));
  store.perturbThetaText = String(p.perturbThetaSigma);
  store.miswriteRate = Math.round(num(p.miswriteRate, 0) * 1000) / 10;
  store.miswriteModeIndex = p.miswriteModeIndex;
  store.strikeoutStyleIndex = p.miswriteStrikeoutStyleIndex;
  store.fontColor = normalizeHex(p.fill);
  store.marginTop = Math.round(num(p.marginTop, 30));
  store.marginBottom = Math.round(num(p.marginBottom, 30));
  store.marginLeft = Math.round(num(p.marginLeft, 30));
  store.marginRight = Math.round(num(p.marginRight, 30));
  store.boundsVisible = p.boundsVisible ?? false;
  store.boundsColor = normalizeHex(p.boundsColor || "#4ca6a6");
  store.endChars = p.endChars ?? "，。";
  store.startChars = p.startChars ?? "";
  if (p.roles && p.roles.length > 0) {
    const nextRoles = p.roles.map((r) => ({ ...r }));
    // 当前段落 runs / regions 引用、但预设里没有的角色需要保留，
    // 否则这些引用会静默回退到默认角色
    const referenced = new Set<number>();
    for (const para of store.paragraphs) {
      for (const run of para.runs ?? []) {
        const rid = run.style?.roleId ?? 0;
        if (rid >= 2) referenced.add(rid);
      }
    }
    for (const region of store.regions) {
      const rid = region.roleId ?? 0;
      if (rid >= 2) referenced.add(rid);
    }
    for (const rid of referenced) {
      if (!nextRoles.some((r) => r.id === rid)) {
        const existing = store.roles.find((r) => r.id === rid);
        if (existing) nextRoles.push({ ...existing });
      }
    }
    nextRoles.sort((a, b) => a.id - b.id);
    store.roles = nextRoles;
  }
  store.docPages = null;
  store.docStatus = "";
  // 预设会同时换纸张：旧底图上的区域坐标随之失效，一并清掉（载入前已确认）
  store.regions.splice(0);
  store.regionMode = false;
  store.editingIndex = -1;
  store.highlightIndex = -1;
  store.selectedRegionIndex = -1;
  // 图形与区域共用背景坐标：换纸张后同样失效，清空并退出图形标注模式
  store.shapes.splice(0);
  store.shapeMode = false;
  store.selectedShapeIndex = -1;
  store.currentPresetPath = meta?.path ?? "";
  store.currentPresetName = meta?.name ?? "";
  store.presetDirty = false;
  if (msg) store.status = msg;
  // applyPreset 自身会触发参数快照 watch；在 nextTick 里复位，避免刚载入就显示「已修改」
  void nextTick(() => {
    suppressPresetDirty = false;
    store.presetDirty = false;
  });
}

let suppressPresetDirty = false;

/** 预设只关心这些字段；用户改任意一项就标记「已修改」。 */
const presetParamSnapshot = (): string =>
  JSON.stringify({
    fontPath: store.fontPath,
    backgroundPath: store.backgroundPath,
    fontSize: store.fontSize,
    wordSpacing: store.wordSpacing,
    lineSpacing: store.lineSpacing,
    wordSpacingSigma: store.wordSpacingSigma,
    lineSpacingSigma: store.lineSpacingSigma,
    fontSizeSigma: store.fontSizeSigma,
    perturbX: store.perturbX,
    perturbY: store.perturbY,
    perturbThetaText: store.perturbThetaText,
    miswriteRate: store.miswriteRate,
    miswriteModeIndex: store.miswriteModeIndex,
    strikeoutStyleIndex: store.strikeoutStyleIndex,
    fontColor: store.fontColor,
    marginTop: store.marginTop,
    marginBottom: store.marginBottom,
    marginLeft: store.marginLeft,
    marginRight: store.marginRight,
    boundsVisible: store.boundsVisible,
    boundsColor: store.boundsColor,
    endChars: store.endChars,
    startChars: store.startChars,
    roles: store.roles,
  });

watch(presetParamSnapshot, () => {
  if (suppressPresetDirty) return;
  if (store.currentPresetPath || store.currentPresetName) store.presetDirty = true;
});

// 用户手动换字体 / 纸张后，上一条「预设资产缺失」提示就过期了
watch(
  () => `${store.fontPath}\u0000${store.backgroundPath}`,
  () => {
    if (!suppressPresetDirty) store.presetNotice = "";
  },
);

/** 已有内容时，从引导里重选「示例 / 填文档」需要确认，避免覆盖当前工作。 */
function confirmReplaceContent(): Promise<boolean> {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value: boolean) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    appDialog.warning({
      title: "当前已有内容",
      content: "继续会替换或叠加当前内容。建议先导出，或先保存成预设。",
      positiveText: "继续",
      negativeText: "取消",
      onPositiveClick: () => finish(true),
      onNegativeClick: () => finish(false),
      onClose: () => finish(false),
      onMaskClick: () => finish(false),
    });
  });
}

function confirmDiscardDocPages(reason: "preset" | "text" = "preset"): Promise<boolean> {
  const hasContent = !!store.docPages?.length || store.regions.length > 0 || store.shapes.length > 0;
  if (!hasContent) return Promise.resolve(true);
  const toText = reason === "text";
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value: boolean) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    appDialog.warning({
      title: toText ? "切换到文字模式？" : "切换预设会重置文档底图、文字区域与图形",
      content: toText
        ? "当前已导入文档底图、创建文字区域或图形标注。继续会清空这些内容，回到纯文字排版；建议先导出。"
        : "当前已导入文档底图、手动创建文字区域或图形标注。切换预设会清空这些内容，建议先导出或保存当前设置。",
      positiveText: toText ? "切换并编辑" : "继续切换",
      negativeText: "取消",
      onPositiveClick: () => finish(true),
      onNegativeClick: () => finish(false),
      onClose: () => finish(false),
      onMaskClick: () => finish(false),
    });
  });
}

/** 预设里的字体 / 纸张可能在本机不存在：能兜底就兜底，并给出可读提示。 */
async function sanitizePresetAssets(p: UiParams): Promise<{ params: UiParams; warning: string }> {
  const warnings: string[] = [];

  let fontPath = (p.fontPath || "").trim();
  if (!fontPath) {
    fontPath = await ensureUsableFont();
  } else if (!(await api.pathExists(fontPath))) {
    const missing = fileName(fontPath);
    await ensureUsableFont();
    if (store.fontPath.trim()) {
      warnings.push(`字体「${missing}」本机未安装，已沿用当前字体`);
      fontPath = store.fontPath;
    } else {
      warnings.push(`字体「${missing}」本机未安装，请先选择一款字体`);
      fontPath = "";
    }
  }

  let backgroundPath = (p.backgroundPath || "").trim();
  const inDocumentMode = (store.docPages?.length ?? 0) > 0;
  if (backgroundPath && !(await api.pathExists(backgroundPath))) {
    warnings.push("纸张图片缺失，已回退到可用纸张");
    // 文档模式下 store.backgroundPath 是底图 PNG，不能拿它当新预设的纸张
    backgroundPath = inDocumentMode ? "" : store.backgroundPath;
  }
  if (!backgroundPath) backgroundPath = await ensureUsableBackground(inDocumentMode);

  return { params: { ...p, fontPath, backgroundPath }, warning: warnings.join("；") };
}

function normalizeHex(s: string): string {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(s.trim());
  return m ? `#${m[1].toLowerCase()}` : "#000000";
}

export async function refreshPresets(): Promise<void> {
  try {
    store.presets = await api.listPresets();
  } catch {
    store.presets = [];
  }
}

export async function selectPreset(path: string): Promise<void> {
  try {
    if (!(await confirmDiscardDocPages())) return;
    const p = await api.loadPreset(path);
    const safe = await sanitizePresetAssets(p);
    const name =
      store.presets.find((x) => x.path === path)?.name ||
      fileName(path).replace(/\.json$/i, "");
    applyPreset(
      safe.params,
      safe.warning ? `已载入预设：${name}（${safe.warning}）` : `已载入预设：${name}`,
      { path, name },
    );
    store.presetNotice = safe.warning;
    if (store.errorSource === "preset") {
      store.errorBanner = "";
      store.errorKind = "";
      store.errorSource = "";
    }
    scheduleRender();
  } catch (e) {
    store.status = `载入失败：${e}`;
    store.errorBanner = `预设载入失败：${e}`;
    store.errorKind = "other";
    store.errorSource = "preset";
  }
}

export async function loadPresetFromDialog(): Promise<void> {
  const p = await dialogs.pickPreset();
  if (!p) return;
  await selectPreset(p);
}

export async function savePresetToDialog(): Promise<void> {
  const defaultDir = await api.defaultPresetDir().catch(() => ".");
  const target = await dialogs.savePresetAs(defaultDir);
  if (!target) return;
  try {
    await api.savePreset(buildParams(), target);
    store.status = `预设已保存：${target}`;
    if (target.replace(/\\/g, "/").startsWith(defaultDir.replace(/\\/g, "/"))) {
      await refreshPresets();
    }
    store.currentPresetPath = target;
    store.currentPresetName = fileName(target).replace(/\.json$/i, "");
    store.presetDirty = false;
    store.presetNotice = "";
  } catch (e) {
    store.status = `保存失败：${e}`;
  }
}

// ---------------------------------------------------------------- 导出

export async function exportFiles(): Promise<void> {
  const dir = await dialogs.pickFolder("选择导出目录");
  if (typeof dir !== "string") return;
  store.status = "导出中…";
  try {
    const files = await api.exportFiles(buildParams(), dir);
    store.status = `已导出 ${files.length} 个文件到 ${dir}`;
    markExported();
    if (store.errorSource === "export") {
      store.errorBanner = "";
      store.errorKind = "";
      store.errorSource = "";
    }
  } catch (e) {
    store.status = `导出失败：${e}`;
    store.errorBanner = `导出失败：${e}`;
    store.errorKind = "other";
    store.errorSource = "export";
  }
}

export async function exportPdf(): Promise<void> {
  const path = await dialogs.savePdf();
  if (typeof path !== "string") return;
  store.status = "导出中…";
  try {
    await api.exportPdf(buildParams(), path);
    store.status = `PDF 已导出：${path}`;
    markExported();
    if (store.errorSource === "export") {
      store.errorBanner = "";
      store.errorKind = "";
      store.errorSource = "";
    }
  } catch (e) {
    store.status = `导出 PDF 失败：${e}`;
    store.errorBanner = `导出 PDF 失败：${e}`;
    store.errorKind = "other";
    store.errorSource = "export";
  }
}

// ---------------------------------------------------------------- 逐段编辑器

export function curParaIndex(): number {
  const i = store.paragraphs.findIndex((p) => p.id === store.curParaId);
  return i >= 0 ? i : store.paragraphs.length - 1;
}

export function updateParaStatus(): void {
  const len = store.paragraphs.length;
  if (len === 0) {
    store.paraStatus = "光标定位到段落后可用按钮设置格式";
    return;
  }
  const idx = Math.min(curParaIndex(), len - 1);
  const row = store.paragraphs[idx];
  if (!row) return;
  const alignName = ["左对齐", "居中", "右对齐"][row.align];
  const px = Math.round(row.indentEm * num(store.fontSize, 36));
  const emTxt =
    Math.abs(row.indentEm - Math.round(row.indentEm)) < 0.01
      ? String(Math.round(row.indentEm))
      : row.indentEm.toFixed(1);
  const indentTxt = row.indentEm > 0 ? `，首行缩进 ${emTxt} 字（${px}px）` : "";
  const text = cleanText(row.text).replace(/\n/g, "");
  const segTxt = text.trim() === "" ? "（空段）" : "";
  store.paraStatus = `第 ${idx + 1} 段（${[...text].length} 字）：${alignName}${indentTxt}${segTxt}`;
}

export function setParagraphs(list: Para[]): void {
  store.paragraphs = list.length ? list : [newPara()];
  store.curParaId = store.paragraphs[0]?.id ?? null;
  updateParaStatus();
}

/** 获取当前选区覆盖的段落 ID 列表（若未多选则返回当前聚焦段 ID） */
export function getSelectedParaIds(): number[] {
  if (typeof window === "undefined") return [];
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) {
    const cur = store.paragraphs[curParaIndex()];
    return cur ? [cur.id] : [];
  }
  const range = sel.getRangeAt(0);
  const editor = document.querySelector(".para-editor");
  if (!editor || !editor.contains(range.commonAncestorContainer)) {
    const cur = store.paragraphs[curParaIndex()];
    return cur ? [cur.id] : [];
  }
  const rows = Array.from(editor.querySelectorAll<HTMLElement>(".para-row"));
  const selected: number[] = [];
  for (const row of rows) {
    if (sel.containsNode(row, true) || range.intersectsNode(row)) {
      const id = Number(row.dataset.id);
      if (id) selected.push(id);
    }
  }
  if (selected.length > 0) return selected;
  const cur = store.paragraphs[curParaIndex()];
  return cur ? [cur.id] : [];
}

export function setAlign(align: number): void {
  const ids = getSelectedParaIds();
  if (ids.length > 0) {
    for (const id of ids) {
      const row = store.paragraphs.find((p) => p.id === id);
      if (row) {
        row.align = (Math.max(0, Math.min(2, align)) as 0 | 1 | 2);
      }
    }
  } else {
    const row = store.paragraphs[curParaIndex()];
    if (row) row.align = (Math.max(0, Math.min(2, align)) as 0 | 1 | 2);
  }
  updateParaStatus();
  scheduleRender();
}

export function toggleIndent(on: boolean): void {
  const ids = getSelectedParaIds();
  if (ids.length > 0) {
    for (const id of ids) {
      const row = store.paragraphs.find((p) => p.id === id);
      if (row) {
        row.indentEm = on ? 2.0 : 0.0;
      }
    }
  } else {
    const row = store.paragraphs[curParaIndex()];
    if (row) row.indentEm = on ? 2.0 : 0.0;
  }
  updateParaStatus();
  scheduleRender();
}

let activeRoleApplier: ((roleId: number) => void) | null = null;

export function registerRoleApplier(fn: (roleId: number) => void): () => void {
  activeRoleApplier = fn;
  return () => {
    if (activeRoleApplier === fn) activeRoleApplier = null;
  };
}

export function applyRoleToSelection(roleId: number): void {
  if (activeRoleApplier) {
    activeRoleApplier(roleId);
  }
}



export function splitPara(id: number, caretOffset: number): void {
  const idx = store.paragraphs.findIndex((p) => p.id === id);
  if (idx < 0) return;
  const row = store.paragraphs[idx];
  const pos = Math.max(0, Math.min(caretOffset, [...row.text].length));
  const chars = [...row.text];
  const before = chars.slice(0, pos).join("");
  const after = chars.slice(pos).join("");
  row.text = before;
  const next = newPara(after, row.align, row.indentEm);
  store.paragraphs.splice(idx + 1, 0, next);
  store.curParaId = next.id;
  updateParaStatus();
  focusPara(next.id, 0);
}

export function mergePrev(id: number): void {
  const idx = store.paragraphs.findIndex((p) => p.id === id);
  if (idx <= 0) return;
  const prev = store.paragraphs[idx - 1];
  const cur = store.paragraphs[idx];
  const joinedLen = [...prev.text].length;
  prev.text = prev.text + cur.text;
  store.paragraphs.splice(idx, 1);
  store.curParaId = prev.id;
  updateParaStatus();
  focusPara(prev.id, joinedLen);
}

/** 粘贴多行：当前段在光标处拆开，多行内容继承当前段格式插入。 */
export function pasteMulti(id: number, caretOffset: number, text: string): void {
  const parts = text.split(/\r?\n/);
  if (parts.length <= 1) return;
  const idx = store.paragraphs.findIndex((p) => p.id === id);
  if (idx < 0) return;
  const row = store.paragraphs[idx];
  const off = Math.max(0, Math.min(caretOffset, [...row.text].length));
  const chars = [...row.text];
  const head = chars.slice(0, off).join("");
  const tail = chars.slice(off).join("");
  row.text = head + cleanText(parts[0]);
  let lastId = row.id;
  for (let k = 1; k < parts.length; k++) {
    const isLast = k === parts.length - 1;
    const content = cleanText(parts[k]) + (isLast ? tail : "");
    const np = newPara(content, row.align, row.indentEm);
    store.paragraphs.splice(idx + k, 0, np);
    lastId = np.id;
  }
  store.curParaId = lastId;
  updateParaStatus();
  focusPara(lastId, [...tail].length);
}

export function focusEmptyArea(): void {
  const len = store.paragraphs.length;
  if (len === 0) return;
  store.curParaId = store.paragraphs[len - 1].id;
  updateParaStatus();
  focusPara(store.paragraphs[len - 1].id, [...store.paragraphs[len - 1].text].length);
}

/** 编辑器消费的焦点请求（id + 光标偏移 + nonce 触发）。 */
export const pendingFocus = reactive({ id: 0, offset: 0, nonce: 0 });

export function focusPara(id: number, offset = 0): void {
  pendingFocus.id = id;
  pendingFocus.offset = offset;
  pendingFocus.nonce++;
}

// ---------------------------------------------------------------- 框选区域

/** 区域是否有逐区域覆盖项（对齐 core TextRegion::has_overrides）。 */
export function regionHasOverrides(r: Region): boolean {
  return (
    r.wordSpacing != null ||
    r.lineSpacing != null ||
    r.fontSizeSigma != null ||
    r.wordSpacingSigma != null ||
    r.lineSpacingSigma != null ||
    r.perturbXSigma != null ||
    r.perturbYSigma != null ||
    r.perturbThetaSigma != null ||
    r.miswriteRate != null ||
    r.miswriteStrikeoutStyleIndex != null ||
    r.fill != null ||
    (r.marginTop != null && r.marginTop > 0) ||
    (r.marginBottom != null && r.marginBottom > 0) ||
    (r.marginLeft != null && r.marginLeft > 0) ||
    (r.marginRight != null && r.marginRight > 0)
  );
}

/** 区域列表摘要（对齐 models.rs TextRegion::label；自定义过追加 ⚙ 标记，
 *  表格单元格区域前缀 表N· 标注归属）。 */
export function regionLabel(r: Region, index: number): string {
  let style = r.printed ? "🖨️ 打印" : "✍️ 手写";
  if (r.roleId && r.roleId >= 2) {
    const role = store.roles.find((role) => role.id === r.roleId);
    if (role) {
      const badge = getRoleBadgeInfo(role);
      style = `${badge.icon} ${role.name}`;
    } else {
      const hlInfo = getHighlightInfo(r.highlight);
      const icon = hlInfo?.icon || "✍️";
      style = `${icon} 手写角色 ${r.roleId - 1}${hlInfo ? ` (${hlInfo.name})` : ""}`;
    }
  }
  const page = r.page > 1 ? ` 第${r.page}页` : "";
  const custom = regionHasOverrides(r) ? " ⚙" : "";
  const tablePrefix = r.tableIndex ? `表${r.tableIndex}·` : "";
  return `${tablePrefix}${index}. ${style}${page} ${[...r.text].length}字 (${r.x},${r.y} ${r.w}×${r.h})${custom}`;
}

/** 把矩形钳制到背景范围并保证最小尺寸（对齐原 clamp_rect）。
 *  全部返回整数：serde 侧区域坐标是 i32，浮点会直接被拒。 */
export function clampRect(
  x: number,
  y: number,
  w: number,
  h: number,
  bw: number,
  bh: number,
): [number, number, number, number] | null {
  if (bw <= 8 || bh <= 8) return null;
  const cw = Math.max(8, Math.round(w));
  const chh = Math.max(8, Math.round(h));
  const cx = Math.max(0, Math.min(Math.round(x), bw - 8));
  const cy = Math.max(0, Math.min(Math.round(y), bh - 8));
  return [
    cx,
    cy,
    Math.max(1, Math.min(cw, bw - cx)),
    Math.max(1, Math.min(chh, bh - cy)),
  ];
}

export function setRegionMode(on: boolean): void {
  store.regionMode = on;
  // 对齐 Python 版 set_region_mode：进入框选模式时结束进行中的区域调整
  if (on) {
    store.editingIndex = -1;
    // 与图形标注互斥：进入框选文字时退出图形模式并清空图形选中
    store.shapeMode = false;
    store.selectedShapeIndex = -1;
  }
}

/** 区域对话框草稿默认值。 */
const defaultRegionDraft = (page: number): Region => ({
  x: 0,
  y: 0,
  w: 0,
  h: 0,
  text: "",
  roleId: 0,
  highlight: null,
  fontPath: "",
  printed: false,
  fontSize: 0,
  page: Math.max(1, page),
  align: 0,
  indentEm: 0,
  paragraphs: [{ text: "", align: 0, indentEm: 0 }],
  marginTop: null,
  marginBottom: null,
  marginLeft: null,
  marginRight: null,
});

/** 草稿有效字号（区域字号 > 主设置），供缩进换算与 docx 导入。 */
function draftFontSize(d: Region): number {
  return d.fontSize > 0 ? d.fontSize : num(store.fontSize, 36);
}

/** 新框选完成 → 暂存矩形并打开属性对话框（起始页 = 当前查看页）。 */
export function openNewRegionDialog(rect: [number, number, number, number]): void {
  store.pendingRect = rect;
  store.dialogTargetIndex = -1;
  store.dialogDraft = defaultRegionDraft(store.pageIndex + 1);
  store.dialogOpen = true;
}

/** 双击/编辑区域 → 打开对话框并回填现有值。 */
export function openEditRegionDialog(index: number): void {
  const r = store.regions[index];
  if (!r) return;
  store.dialogTargetIndex = index;
  const draft: Region = { ...r };
  if (draft.roleId === undefined) {
    draft.roleId = draft.printed ? 1 : 0;
  }
  if (!draft.paragraphs || draft.paragraphs.length === 0) {
    if (draft.text) {
      draft.paragraphs = draft.text.split("\n").map((line) => ({
        text: line,
        align: draft.align ?? 0,
        indentEm: draft.indentEm ?? 0,
      }));
    } else {
      draft.paragraphs = [{ text: "", align: draft.align ?? 0, indentEm: draft.indentEm ?? 0 }];
    }
  } else {
    draft.paragraphs = draft.paragraphs.map((p) => ({ ...p }));
  }
  store.dialogDraft = draft;
  store.dialogOpen = true;
}

export function cancelRegionDialog(): void {
  store.dialogOpen = false;
  store.pendingRect = null;
  store.dialogDraft = null;
}

/** 确定对话框：新建（用暂存矩形）或写回草稿；文字为空则放弃该区域。 */
export function confirmRegionDialog(): void {
  const d = store.dialogDraft;
  if (!d) return;
  if (d.paragraphs && d.paragraphs.length > 0) {
    d.text = d.paragraphs.map((p) => cleanText(p.text)).join("\n");
    d.align = d.paragraphs[0]?.align ?? 0;
    d.indentEm = d.paragraphs[0]?.indentEm ?? 0;
  }
  const text = d.text.trim();
  if (text === "") {
    cancelRegionDialog();
    store.status = "区域文字为空，已放弃该区域";
    return;
  }
  d.text = text.trim();
  d.fontPath = d.fontPath.trim();
  d.fontSize = Math.round(num(d.fontSize, 0));
  d.page = Math.max(1, Math.round(num(d.page, 1)));
  if (store.dialogTargetIndex < 0) {
    const rect = store.pendingRect;
    if (rect) {
      [d.x, d.y, d.w, d.h] = rect;
      store.regions.push({ ...d });
    }
  } else {
    const r = store.regions[store.dialogTargetIndex];
    if (r) Object.assign(r, { ...d });
  }
  cancelRegionDialog();
  scheduleRender();
}

/** 对话框内选择打印字体。 */
export async function chooseRegionFont(): Promise<void> {
  const d = store.dialogDraft;
  if (!d) return;
  const p = await dialogs.pickFont();
  if (typeof p === "string") d.fontPath = p;
}

/** 将字体与字号批量应用到某张表格的全部单元格区域。 */
export function applyFontToTable(tableIndex: number, fontPath: string, fontSize: number): void {
  let n = 0;
  for (const r of store.regions) {
    if (r.tableIndex === tableIndex) {
      r.fontPath = fontPath;
      r.fontSize = fontSize;
      n++;
    }
  }
  store.status = `已将字体应用到表格 ${tableIndex} 的 ${n} 个单元格`;
  scheduleRender();
}

/** 导入 docx 到对话框草稿：保留各段独立对齐与缩进。 */
export async function importDocxToDraft(): Promise<void> {
  const d = store.dialogDraft;
  if (!d) return;
  const p = await dialogs.pickDocx();
  if (!p) return;
  try {
    if (store.systemFonts.length === 0) {
      await initSystemFonts();
    }
    const res = await api.importDocx(p, draftFontSize(d));
    const rows = res.paragraphs;
    if (!rows.length) throw new Error("文档为空");

    const hasHighlightsInDoc = rows.some((row) =>
      row.runs?.some(
        (r) =>
          r.style?.highlight != null ||
          ((r.style?.roleId ?? 0) >= 2 && !r.style?.printed),
      ),
    );

    let useHighlights = false;
    if (hasHighlightsInDoc) {
      const mode = await askDocxHighlightMode();
      if (mode === "cancel") return;
      useHighlights = mode === "mixed";
    }

    d.paragraphs = rows.map((row) => ({
      text: cleanText(row.text),
      align: (row.align ?? 0) as 0 | 1 | 2,
      indentEm: row.indentEm ?? 0,
      runs: row.runs?.map((r) => {
        const runStyle = r.style ? { ...r.style } : undefined;
        if (useHighlights) {
          if (runStyle && (runStyle.roleId === 1 || runStyle.printed)) {
            if (runStyle.fontFamily) {
              const matched = matchSystemFont(runStyle.fontFamily);
              if (matched) {
                runStyle.fontPath = matched.path;
              }
            }
          }
        } else {
          if (runStyle) {
            runStyle.roleId = 0;
            runStyle.printed = false;
            runStyle.fontPath = undefined;
            runStyle.highlight = undefined;
          }
        }
        return {
          text: cleanText(r.text),
          style: runStyle,
        };
      }),
    }));
    d.text = d.paragraphs.map((r) => r.text).join("\n");
    d.align = d.paragraphs[0]?.align ?? 0;
    d.indentEm = d.paragraphs[0]?.indentEm ?? 0;
    store.status = `已导入 ${rows.length} 个段落到区域`;
  } catch (e) {
    store.status = `区域导入 docx 失败：${e}`;
    appDialog.error({ title: "区域导入 docx 失败", content: String(e), positiveText: "知道了" });
  }
}

/** 二次调整写回（rect 已按背景坐标钳制）。 */
export function updateRegionGeometry(index: number, rect: [number, number, number, number]): void {
  const r = store.regions[index];
  if (!r) return;
  [r.x, r.y, r.w, r.h] = rect;
  scheduleRender();
}

export function cancelRegionEdit(): void {
  store.editingIndex = -1;
  store.selectedRegionIndex = -1;
  store.highlightIndex = -1;
}

/** 列表点击：跳到该页并进入调整态。 */
export function jumpToRegion(index: number): void {
  const r = store.regions[index];
  if (!r) return;
  const target = Math.max(0, r.page - 1);
  if (target !== store.pageIndex && target < store.previewPages.length) {
    store.pageIndex = target;
  }
  store.selectedRegionIndex = index;
  store.editingIndex = index;
  // 区域与图形互斥选中：否则跨页后会留下「看不见但可被 Delete」的幽灵图形
  store.selectedShapeIndex = -1;
  store.shapeMode = false;
}

/** 列表「🔍」：跳到该区域所在页、选中，并请求预览居中放大到该区域。 */
export function focusRegion(index: number): void {
  const r = store.regions[index];
  if (!r) return;
  jumpToRegion(index);
  store.previewFocusIndex = index;
  store.previewFocusToken += 1;
}

export function deleteSelectedRegion(): void {
  const i = store.selectedRegionIndex;
  if (i >= 0 && i < store.regions.length) store.regions.splice(i, 1);
  store.editingIndex = -1;
  store.highlightIndex = -1;
  store.selectedRegionIndex = -1;
  scheduleRender();
}

export function clearRegions(): void {
  store.regions.splice(0);
  store.editingIndex = -1;
  store.highlightIndex = -1;
  store.selectedRegionIndex = -1;
  // 注意：**不清图形**。清空文字区域既不换纸张也不改坐标，图形仍然有效；
  // 图形由「④ 图形标注 → 清空」单独管理（避免误点一下连标注一起丢）。
  scheduleRender();
}

// ---------------------------------------------------------------- 图形标注（背景原始像素坐标）
// 坐标系与 UiRegion 完全一致：图形几何一律存背景原始像素，预览缩放由 PreviewPane 的 factor 负责。

/** 图形类型的中文名称（列表 / 属性面板 / 下拉框共用）。 */
export const SHAPE_KIND_LABELS: Record<UiShapeKind, string> = {
  line: "直线",
  arrow: "箭头",
  polyline: "折线",
  rect: "矩形",
  ellipse: "椭圆",
  triangle: "三角形",
  brace_left: "左大括号",
  brace_right: "右大括号",
  bracket_left: "左方括号",
  bracket_right: "右方括号",
  check: "勾",
  underline: "下划线",
};

/** 细长图形：命中测试按「点到线段距离」判定、交互用端点手柄而不是 8 向手柄。 */
export const LINE_LIKE_SHAPE_KINDS: ReadonlySet<UiShapeKind> = new Set<UiShapeKind>([
  "line",
  "arrow",
  "underline",
]);

/** 把 [x0,y0,x1,y1] 归一化为左上角 + 正宽高（用户可从任意方向拖拽）。 */
function normalizedShapeBox(
  x0: number,
  y0: number,
  x1: number,
  y1: number,
): { x: number; y: number; w: number; h: number } {
  return {
    x: Math.min(x0, x1),
    y: Math.min(y0, y1),
    w: Math.abs(x1 - x0),
    h: Math.abs(y1 - y0),
  };
}

/** 生成随机 uint32 种子：同一图形的手绘抖动形态由它固定。 */
function randomShapeSeed(): number {
  return Math.floor(Math.random() * 2 ** 32);
}

/** 点到线段的最短距离（背景原始像素）。 */
function pointSegmentDistance(
  px: number,
  py: number,
  x0: number,
  y0: number,
  x1: number,
  y1: number,
): number {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const lenSq = dx * dx + dy * dy;
  if (lenSq <= 1e-9) return Math.hypot(px - x0, py - y0);
  const t = Math.max(0, Math.min(1, ((px - x0) * dx + (py - y0) * dy) / lenSq));
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}

/** 线/箭头/下划线的两个端点：优先 points 前两点，否则用包围盒对角线（与 core 一致）。 */
function shapeEndpoints(s: UiShape): [[number, number], [number, number]] {
  const pts = s.points;
  if (pts && pts.length >= 2) {
    return [
      [pts[0][0], pts[0][1]],
      [pts[1][0], pts[1][1]],
    ];
  }
  return [
    [s.x, s.y],
    [s.x + s.w, s.y + s.h],
  ];
}

/** 点到「包围盒四条边」的最短距离（用于空心矩形 / 大括号 / 方括号的命中）。 */
function distanceToBoxBorder(
  box: { x: number; y: number; w: number; h: number },
  px: number,
  py: number,
): number {
  const dx = Math.max(box.x - px, px - (box.x + box.w), 0);
  const dy = Math.max(box.y - py, py - (box.y + box.h), 0);
  // 在框内：到最近边的距离；在框外：到框的欧氏距离
  if (dx === 0 && dy === 0) {
    return Math.min(
      Math.abs(px - box.x),
      Math.abs(px - (box.x + box.w)),
      Math.abs(py - box.y),
      Math.abs(py - (box.y + box.h)),
    );
  }
  return Math.hypot(dx, dy);
}

/** 点到椭圆描边（环）的近似像素距离：|归一化半径 - 1| × min(rx, ry)。 */
function distanceToEllipseRing(
  box: { x: number; y: number; w: number; h: number },
  px: number,
  py: number,
): number {
  const rx = Math.max(box.w / 2, 1e-3);
  const ry = Math.max(box.h / 2, 1e-3);
  const cx = box.x + rx;
  const cy = box.y + ry;
  const r = Math.hypot((px - cx) / rx, (py - cy) / ry);
  return Math.abs(r - 1) * Math.min(rx, ry);
}

/** 三角形三条边的最近距离：顶点与 core 一致（上中 / 左下 / 右下），
 *  显式 points 给了 3 点时用指定顶点。 */
function distanceToTriangleEdges(
  s: UiShape,
  box: { x: number; y: number; w: number; h: number },
  px: number,
  py: number,
): number {
  const pts = s.points;
  const v: [number, number][] =
    pts && pts.length >= 3
      ? [
          [pts[0][0], pts[0][1]],
          [pts[1][0], pts[1][1]],
          [pts[2][0], pts[2][1]],
        ]
      : [
          [box.x + box.w / 2, box.y],
          [box.x, box.y + box.h],
          [box.x + box.w, box.y + box.h],
        ];
  let best = Infinity;
  for (let k = 0; k < 3; k++) {
    const a = v[k];
    const b = v[(k + 1) % 3];
    best = Math.min(best, pointSegmentDistance(px, py, a[0], a[1], b[0], b[1]));
  }
  return best;
}

/** 勾（✓）两段折线的最近距离：与 core 的顶点公式一致。 */
function distanceToCheckStrokes(
  box: { x: number; y: number; w: number; h: number },
  px: number,
  py: number,
): number {
  const a: [number, number] = [box.x, box.y + box.h * 0.55];
  const b: [number, number] = [box.x + box.w * 0.36, box.y + box.h * 0.95];
  const c: [number, number] = [box.x + box.w, box.y + box.h * 0.05];
  return Math.min(
    pointSegmentDistance(px, py, a[0], a[1], b[0], b[1]),
    pointSegmentDistance(px, py, b[0], b[1], c[0], c[1]),
  );
}

/** 大括号的笔迹折线近似：与 core `shapes.rs::brace()` 一致——
 *  从近侧边起笔 → 竖脊（55% 宽处）→ 中间尖角 → 竖脊 → 近侧边收笔。
 *  只用包围盒四边近似会漏掉竖脊（点在脊上选不中）。 */
function distanceToBraceStrokes(
  box: { x: number; y: number; w: number; h: number },
  side: "left" | "right",
  px: number,
  py: number,
): number {
  const dir = side === "right" ? 1 : -1;
  const nearX = dir > 0 ? box.x : box.x + box.w;
  const cuspX = dir > 0 ? box.x + box.w : box.x;
  const spineX = nearX + dir * box.w * 0.55;
  const pts: [number, number][] = [
    [nearX, box.y],
    [spineX, box.y + box.h * 0.05],
    [spineX, box.y + box.h * 0.45],
    [cuspX, box.y + box.h * 0.5],
    [spineX, box.y + box.h * 0.55],
    [spineX, box.y + box.h * 0.95],
    [nearX, box.y + box.h],
  ];
  let best = Infinity;
  for (let k = 0; k + 1 < pts.length; k++) {
    best = Math.min(
      best,
      pointSegmentDistance(px, py, pts[k][0], pts[k][1], pts[k + 1][0], pts[k + 1][1]),
    );
  }
  return best;
}

/** 进入/退出图形标注模式（与 regionMode 互斥，两种模式都清空选中）。 */
export function setShapeMode(on: boolean): void {
  store.shapeMode = on;
  if (on) {
    // 与框选文字互斥：进入图形标注时退出区域模式及其调整态
    store.regionMode = false;
    store.editingIndex = -1;
  }
  store.selectedShapeIndex = -1;
}

/** 设置当前要画的图元类型。 */
export function setShapeKind(k: UiShapeKind): void {
  store.shapeKind = k;
}

/** 当前页的图形命中测试：返回图形索引（-1 = 未命中）。
 *  数组顺序 = 绘制顺序，从后往前遍历（后画的在上层，先命中）。
 *
 *  `tolSrc` 是**背景原始像素**容差，由调用方按显示比例换算
 *  （`PreviewPane` 用 `8 / factor`，保证不同缩放下点选手感一致）。
 *
 *  空心图形只命中描边：否则在文字上画一个大方框后，框内任意位置都会选中这个框，
 *  文字区域就再也点不到了（图形优先于区域）。实心图形才按内部命中。 */
export function shapeAtPoint(srcX: number, srcY: number, tolSrc = 3): number {
  const tol = Math.max(1, tolSrc);
  for (let i = store.shapes.length - 1; i >= 0; i--) {
    const s = store.shapes[i];
    // 图形按页归属：只命中当前页，避免选中看不见的图形
    if (s.page !== store.pageIndex + 1) continue;
    if (LINE_LIKE_SHAPE_KINDS.has(s.kind)) {
      // 细线按点到线段距离判定；容差取「显示容差」与「半线宽」的较大者
      //（半线宽而不是 2×线宽：8px 粗线在 400% 下 2×线宽会变成约 64 CSS px 的巨大容差）
      const [[x0, y0], [x1, y1]] = shapeEndpoints(s);
      const lineTol = Math.max(tol, s.thickness * 0.5);
      if (pointSegmentDistance(srcX, srcY, x0, y0, x1, y1) <= lineTol) return i;
      continue;
    }
    const box = normalizedShapeBox(s.x, s.y, s.x + s.w, s.y + s.h);
    const insideBox =
      srcX >= box.x - tol &&
      srcX <= box.x + box.w + tol &&
      srcY >= box.y - tol &&
      srcY <= box.y + box.h + tol;
    if (!insideBox) continue;
    // 实心图形：内部即命中
    if (s.filled) return i;
    // 空心图形：只认描边附近（按各图元的真实笔迹求距离，避免误吞内部点击；
    // 大括号用「竖脊 + 尖角」折线，不能用包围盒四边近似——会漏掉 55% 宽处的脊）
    let dist: number;
    switch (s.kind) {
      case "ellipse":
        dist = distanceToEllipseRing(box, srcX, srcY);
        break;
      case "triangle":
        dist = distanceToTriangleEdges(s, box, srcX, srcY);
        break;
      case "check":
        dist = distanceToCheckStrokes(box, srcX, srcY);
        break;
      case "brace_left":
        dist = distanceToBraceStrokes(box, "left", srcX, srcY);
        break;
      case "brace_right":
        dist = distanceToBraceStrokes(box, "right", srcX, srcY);
        break;
      default:
        dist = distanceToBoxBorder(box, srcX, srcY);
        break;
    }
    if (dist <= Math.max(tol, s.thickness * 0.5)) return i;
  }
  return -1;
}

/** 新建图形的默认线宽：按纸张原始像素宽度自适应。
 *  固定 2.4px 在 4200×5930 的扫描纸上、fit 视图（≈12.5%）只有 0.3 显示像素，
 *  用户刚画完会以为「没画上 / 没生效」。 */
function defaultThickness(): number {
  const w = store.bgNatW;
  if (!w || w <= 0) return 2.4;
  return Math.min(12, Math.max(2.4, Math.round(w / 700)));
}

/** 数值夹取（lo <= v <= hi）。 */
function clampRange(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/**
 * 设置「新建图形默认样式」。
 * 传 null（或省略该键）表示恢复自动；颜色统一规范化成 #RRGGBB（小写）。
 * 只影响之后新画的图形，不触碰已画图形；也不需要重渲染。
 */
export function setShapeDefault(patch: Partial<ShapeDefaultStyle>): void {
  const d = store.shapeDefault;
  if ("fill" in patch) {
    d.fill = patch.fill ? normalizeHex(patch.fill) : null;
  }
  if ("thickness" in patch) {
    const v = patch.thickness;
    d.thickness = typeof v === "number" && Number.isFinite(v) ? clampRange(v, 1, 16) : null;
  }
  if ("jitter" in patch) {
    const v = patch.jitter;
    d.jitter = typeof v === "number" && Number.isFinite(v) ? clampRange(v, 0, 6) : null;
  }
}

/**
 * 解析新建图形实际生效的样式：
 * - thickness：手填优先，否则按纸张宽度自适应（defaultThickness）；
 * - jitter：手填优先，否则取 thickness × 0.9 并夹到 1.4~6 ——
 *   纸越大线越粗、抖动越小越像尺子，这个比例能保证大纸上也有手绘感；
 * - fill：手填颜色；null = 跟随主墨色（交给引擎用主墨色绘制）。
 */
export function resolveShapeDefault(): { fill: string | null; thickness: number; jitter: number } {
  const thickness = store.shapeDefault.thickness ?? defaultThickness();
  const jitter = store.shapeDefault.jitter ?? clampRange(thickness * 0.9, 1.4, 6);
  const fill = store.shapeDefault.fill ? normalizeHex(store.shapeDefault.fill) : null;
  return { fill, thickness, jitter };
}

/** 新建图形：rect = [x0,y0,x1,y1]（可反向拖拽，内部归一化为正宽高）。
 *  points 为背景原始像素的源坐标（line/arrow 用于保留方向）。
 *  返回新图形索引；宽高都 < 2px 的误触返回 -1 且不改动 store。 */
export function addShapeFromRect(
  kind: UiShapeKind,
  rect: [number, number, number, number],
  points?: [number, number][],
): number {
  const box = normalizedShapeBox(rect[0], rect[1], rect[2], rect[3]);
  if (box.w < 2 && box.h < 2) return -1;
  const style = resolveShapeDefault();
  store.shapes.push({
    kind,
    page: store.pageIndex + 1,
    x: box.x,
    y: box.y,
    w: box.w,
    h: box.h,
    points: points?.map(([px, py]) => [px, py] as [number, number]),
    thickness: style.thickness,
    jitter: style.jitter,
    filled: false,
    fill: style.fill,
    seed: randomShapeSeed(),
  });
  const index = store.shapes.length - 1;
  // 新建后自动选中，方便立刻调属性
  selectShape(index);
  scheduleRender();
  return index;
}

/** 用文字区域的外接矩形一键生成「矩形」标注（跟随区域所在页，不是当前页）。 */
export function addShapeAroundRegion(index: number): void {
  const r = store.regions[index];
  if (!r) return;
  if (![r.x, r.y, r.w, r.h].every((v) => Number.isFinite(v))) return;
  // 空区域（识别失败残留）或完全落在纸张外时跳过
  if (r.w < 2 && r.h < 2) return;
  if (store.bgNatW > 0 && store.bgNatH > 0) {
    const outside =
      r.x >= store.bgNatW || r.y >= store.bgNatH || r.x + r.w <= 0 || r.y + r.h <= 0;
    if (outside) return;
  }
  // 沿区域 bbox 外扩一点：线宽会跨在边界上，零 padding 必然压到最外圈的字
  const pad = Math.round(defaultThickness() / 2 + 4);
  const newIndex = addShapeFromRect("rect", [
    r.x - pad,
    r.y - pad,
    r.x + r.w + pad,
    r.y + r.h + pad,
  ]);
  if (newIndex < 0) return;
  // 区域可能在别的页：图形跟着区域走，避免框到错误页面
  store.shapes[newIndex].page = Math.max(1, Math.round(num(r.page, 1)));
}

/** 写回图形几何（rect 已换算成背景原始像素）。
 *  points 按新旧包围盒比例重算：p' = 新原点 + (p - 旧原点) * 新尺寸 / 旧尺寸，
 *  拖动/缩放后线、箭头的方向与相对位置不丢。 */
export function updateShapeGeometry(
  index: number,
  rect: [number, number, number, number],
): void {
  const s = store.shapes[index];
  if (!s) return;
  const next = normalizedShapeBox(rect[0], rect[1], rect[2], rect[3]);
  const old = normalizedShapeBox(s.x, s.y, s.x + s.w, s.y + s.h);
  // 退化轴（水平线 h≈0 / 垂直线 w≈0）：按比例缩放会把点全贴到新框的上/左边缘，
  // 这里改成映射到**新包围盒的中线**，拖动/缩放后线的位置符合直觉。
  const zeroW = old.w < 0.5;
  const zeroH = old.h < 0.5;
  const sx = next.w / (old.w > 0 ? old.w : 1);
  const sy = next.h / (old.h > 0 ? old.h : 1);
  if (s.points) {
    s.points = s.points.map(([px, py]) => {
      const nx = zeroW ? next.x + next.w / 2 : next.x + (px - old.x) * sx;
      const ny = zeroH ? next.y + next.h / 2 : next.y + (py - old.y) * sy;
      return [nx, ny] as [number, number];
    });
  }
  s.x = next.x;
  s.y = next.y;
  s.w = next.w;
  s.h = next.h;
  scheduleRender();
}

/** 直接写回线类图形（直线 / 箭头 / 下划线）的两个端点（背景原始像素）。
 *  端点拖动专用：同时重算包围盒，保证 points 与 bbox 始终一致；
 *  端点是用户拖出来的真实方向，不能用 bbox 对角线反推（反向拖拽会丢方向）。 */
export function setShapeEndpoints(
  index: number,
  a: [number, number],
  b: [number, number],
): void {
  const s = store.shapes[index];
  if (!s) return;
  const box = normalizedShapeBox(a[0], a[1], b[0], b[1]);
  s.points = [
    [a[0], a[1]],
    [b[0], b[1]],
  ];
  s.x = box.x;
  s.y = box.y;
  s.w = box.w;
  s.h = box.h;
  scheduleRender();
}

/** 修改图形样式（属性面板）：页码变化时同步切到该页，方便在预览中确认。 */
export function updateShapeStyle(index: number, patch: Partial<UiShape>): void {
  const s = store.shapes[index];
  if (!s) return;
  const next: Partial<UiShape> = { ...patch };
  // 深层拷贝 points，避免属性面板的草稿数组被直接挂进 store
  if (next.points) next.points = next.points.map(([px, py]) => [px, py] as [number, number]);
  Object.assign(s, next);
  if (typeof patch.page === "number" && Number.isFinite(patch.page)) {
    s.page = Math.max(1, Math.round(patch.page));
    // 改页码 = 把图形移到该页并切过去，方便在预览里确认。
    // 夹到现有预览页内：目标页还没渲染出来时直接跳会让预览空白一帧。
    store.pageIndex = Math.max(
      0,
      Math.min(s.page - 1, Math.max(store.previewPages.length - 1, 0)),
    );
  }
  scheduleRender();
}

/** 选中图形（-1 / 越界 = 清空选中）。 */
export function selectShape(index: number): void {
  store.selectedShapeIndex = index >= 0 && index < store.shapes.length ? index : -1;
}

/** 清空图形选中（点空白 / 切页 / 退出模式时调用）。 */
export function clearShapeSelection(): void {
  store.selectedShapeIndex = -1;
}

/** 删除单个图形；选中索引随数组收缩修正。 */
export function deleteShape(index: number): void {
  if (index < 0 || index >= store.shapes.length) return;
  store.shapes.splice(index, 1);
  if (store.selectedShapeIndex === index) store.selectedShapeIndex = -1;
  else if (store.selectedShapeIndex > index) store.selectedShapeIndex -= 1;
  scheduleRender();
}

/** 清空全部图形（保留图形标注模式本身）。 */
/** 内部：直接清空图形（不做确认，供已经确认过的调用方复用）。 */
function resetShapes(): void {
  store.shapes.splice(0);
  store.selectedShapeIndex = -1;
  scheduleRender();
}

/** 清空全部图形：带二次确认。
 *  图形没有撤销、也不随预设保存，误点「清空」等于永久丢失整页标注。 */
export async function clearShapes(): Promise<void> {
  if (store.shapes.length === 0) return;
  const ok = await new Promise<boolean>((resolve) => {
    let settled = false;
    const finish = (v: boolean) => {
      if (settled) return;
      settled = true;
      resolve(v);
    };
    appDialog.warning({
      title: "清空全部图形标注？",
      content: `将删除当前文档里的 ${store.shapes.length} 个图形标注，此操作无法撤销（图形也不随预设保存）。`,
      positiveText: "清空",
      negativeText: "取消",
      onPositiveClick: () => finish(true),
      onNegativeClick: () => finish(false),
      onClose: () => finish(false),
      onMaskClick: () => finish(false),
    });
  });
  if (ok) resetShapes();
}

/** 叠放顺序：数组顺序 = 绘制顺序（越靠后越在上层）。
 *  `dir = +1` 表示「更靠上一层」（与列表里的 ↑ 对应），`-1` 表示下移一层。
 *  引擎已按数组顺序逐图形合成（不再按颜色合并），所以这个顺序在预览/导出里真实可见。 */
export function moveShapeOrder(index: number, dir: -1 | 1): void {
  const to = index + dir;
  if (index < 0 || index >= store.shapes.length || to < 0 || to >= store.shapes.length) return;
  const tmp = store.shapes[index];
  store.shapes[index] = store.shapes[to];
  store.shapes[to] = tmp;
  // 选中项跟着图形走，避免上下移后选到隔壁
  if (store.selectedShapeIndex === index) store.selectedShapeIndex = to;
  else if (store.selectedShapeIndex === to) store.selectedShapeIndex = index;
  scheduleRender();
}

/** 图形列表文案，如 "1. 矩形 第1页 2.4px"。 */
export function shapeLabel(s: UiShape, i: number): string {
  return `${i + 1}. ${SHAPE_KIND_LABELS[s.kind] || "图形"} 第${s.page}页 ${s.thickness.toFixed(1)}px`;
}

// ---------------------------------------------------------------- 关于与更新

export function openAboutModal(): void {
  store.aboutModalOpen = true;
}

export function closeAboutModal(): void {
  store.aboutModalOpen = false;
}

export function openHelpModal(tab = "quickstart"): void {
  store.helpTab = tab;
  store.helpModalOpen = true;
}

export function closeHelpModal(): void {
  store.helpModalOpen = false;
}

export function openUpdateModal(info?: UpdateInfo): void {
  if (info) store.updateInfo = info;
  store.updateModalOpen = true;
}

export function closeUpdateModal(skip = false): void {
  store.updateModalOpen = false;
  if (skip && store.updateInfo?.version) {
    setSkippedVersion(store.updateInfo.version);
  }
}

export function setAutoCheckUpdate(enabled: boolean): void {
  store.autoCheckUpdate = enabled;
  try {
    localStorage.setItem(KEY_AUTO_CHECK, String(enabled));
  } catch {}
}

export function setSkippedVersion(version: string): void {
  store.skippedVersion = version;
  try {
    localStorage.setItem(KEY_SKIPPED_VERSION, version);
  } catch {}
}

export async function openExternalUrl(url: string): Promise<void> {
  if (!url) return;
  try {
    await api.openUrl(url);
  } catch (e) {
    console.error("打开浏览器失败:", e);
    // fallback window.open
    window.open(url, "_blank");
  }
}

/** 手动触发检查更新（在关于对话框中） */
export async function manualCheckUpdate(): Promise<void> {
  if (store.checkingUpdate) return;
  store.checkingUpdate = true;
  store.updateStatusText = "正在连接 GitHub 查询最新版本…";

  try {
    const info = await api.checkForUpdates(APP_VERSION);
    store.updateInfo = info;

    if (info.hasUpdate) {
      store.updateStatusText = `🎉 发现新版本：v${info.version}`;
      openUpdateModal(info);
    } else {
      store.updateStatusText = `✅ 当前已是最新版本 (v${APP_VERSION})`;
      appMessage.success(`当前已是最新版本 (v${APP_VERSION})，无需更新。`);
    }
  } catch (e) {
    store.updateStatusText = "❌ 查询失败，请检查网络连接";
    appDialog.warning({
      title: "检查更新失败",
      content: `无法连接至 GitHub Releases API：${e}\n请检查网络连接或稍后重试。`,
      positiveText: "知道了",
    });
  } finally {
    store.checkingUpdate = false;
  }
}

/** 软件启动时静默检查更新（本次会话只跑一次，避免首启补检与正常启动重复） */
let startupUpdateChecked = false;
export async function startupCheckUpdate(): Promise<void> {
  if (!store.autoCheckUpdate || startupUpdateChecked) return;
  startupUpdateChecked = true;

  try {
    const info = await api.checkForUpdates(APP_VERSION);
    store.updateInfo = info;

    if (info.hasUpdate) {
      // 若用户未跳过此版本，则主动弹出更新提示
      if (info.version !== store.skippedVersion) {
        openUpdateModal(info);
      }
    }
  } catch (e) {
    // 启动静默检查失败时不打扰用户
    console.warn("启动检查更新失败:", e);
  }
}

// ---------------------------------------------------------------- 角色管理

export function addRole(name?: string, printed = false, highlight?: string | null): UiHandwritingRole {
  const maxId = store.roles.reduce((m, r) => Math.max(m, r.id), -1);
  const nextId = Math.max(maxId + 1, 2);
  const hlInfo = getHighlightInfo(highlight);
  const hlLabel = hlInfo ? ` (${hlInfo.name})` : "";
  const newRole: UiHandwritingRole = {
    id: nextId,
    name: name || (printed ? `打印角色 ${nextId}` : `手写角色 ${nextId - 1}${hlLabel}`),
    fontPath: "",
    printed,
    highlight: highlight ?? null,
    fill: null,
  };
  store.roles.push(newRole);
  scheduleRender();
  return newRole;
}

export function deleteRole(id: number): void {
  const idx = store.roles.findIndex((r) => r.id === id);
  if (idx >= 0) {
    store.roles.splice(idx, 1);
    scheduleRender();
  }
}

export async function chooseRoleFont(id: number): Promise<void> {
  const role = store.roles.find((r) => r.id === id);
  if (!role) return;
  const p = await dialogs.pickFont();
  if (typeof p === "string") {
    role.fontPath = p;
    scheduleRender();
  }
}

export function resetRoles(): void {
  store.roles = defaultRoles();
  scheduleRender();
}

export function roleHasOverrides(r: UiHandwritingRole): boolean {
  return (
    r.fontSize != null ||
    r.fill != null ||
    (r.highlight != null && r.highlight !== "") ||
    r.wordSpacing != null ||
    r.lineSpacing != null ||
    r.fontSizeSigma != null ||
    r.wordSpacingSigma != null ||
    r.lineSpacingSigma != null ||
    r.perturbXSigma != null ||
    r.perturbYSigma != null ||
    r.perturbThetaSigma != null ||
    r.miswriteRate != null ||
    r.miswriteStrikeoutStyleIndex != null
  );
}


