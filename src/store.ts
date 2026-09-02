//! 全局应用状态（reactive 单例）——对应原 Slint 版 main.rs 的全部 UI 逻辑：
//! 参数收集（buildParams ↔ collect_params）、防抖预览、翻页、预设、
//! 段落结构操作（分段/合并/对齐/缩进）、框选区域增删改、文档底图同步。
//!
//! 渲染代次守卫：renderSeq 只采纳最新一次请求结果，过期响应直接丢弃。

import { reactive, watch } from "vue";
import { createDiscreteApi } from "naive-ui";
import { useDebounceFn } from "@vueuse/core";
import { api, assetUrl, dialogs } from "./api";
import type { SystemFontItem, UiHandwritingRole, UiParams, UiRegion, UiTextRun, UpdateInfo } from "./types";

/** 当前版本号与开源仓库地址 */
export const APP_VERSION = "0.3.3";

export const PYTHON_REPO_URL = "https://github.com/bamboostrip/Handwriting-simulator";
export const RUST_REPO_URL = "https://github.com/bamboostrip/Handwriting-sim-rs";

const KEY_AUTO_CHECK = "handwritesim_auto_check";
const KEY_SKIPPED_VERSION = "handwritesim_skipped_version";
const KEY_THEME_PREFERENCE = "handwritesim_theme_preference";

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

  // ---- 框选文字区域 ----
  regionMode: false,
  regions: [] as Region[],
  /** 当前内嵌编辑的区域索引（-1 = 无；预览上的调整框与右侧编辑卡片共用） */
  editingIndex: -1,
  highlightIndex: -1,
  selectedRegionIndex: -1,

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
  bgNatW: 0,
  bgNatH: 0,
  rendering: false,
  status: "就绪",

  presets: [] as { name: string; path: string }[],

  // ---- 关于与版本更新 ----
  aboutModalOpen: false,
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
    roles: store.roles.map((r) => ({ ...r })),
    endChars: store.endChars,
    startChars: store.startChars,
    boundsVisible: store.boundsVisible,
    boundsColor: store.boundsColor,
  };
}

// ---------------------------------------------------------------- 预览渲染

let renderSeq = 0;

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
  } catch (e) {
    if (seq !== renderSeq) return;
    store.status = `渲染失败：${e}`;
  } finally {
    if (seq === renderSeq) store.rendering = false;
  }
}

const debouncedRender = useDebounceFn(doRender, 300);
export const scheduleRender = (): void => void debouncedRender();
export const triggerPreview = scheduleRender;

export function prevPage(): void {
  if (store.pageIndex > 0) store.pageIndex -= 1;
}

export function nextPage(): void {
  if (store.pageIndex + 1 < store.previewPages.length) store.pageIndex += 1;
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

export async function initSystemFonts(): Promise<void> {
  try {
    store.systemFonts = await api.listSystemFonts();
  } catch (e) {
    console.warn("获取系统字体列表失败:", e);
    store.systemFonts = [];
  }
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
  const p = await dialogs.pickImage();
  if (typeof p === "string") store.backgroundPath = p;
}

export async function importDocx(): Promise<void> {
  const p = await dialogs.pickDocx();
  if (!p) return;
  try {
    if (store.systemFonts.length === 0) {
      await initSystemFonts();
    }
    const res = await api.importDocx(p, num(store.fontSize, 36));
    const rows = res.paragraphs;

    // 检查文档是否包含高亮或独立角色标记（混合模式 vs 纯手写模式）
    const hasHighlights = rows.some((row) =>
      row.runs?.some(
        (r) =>
          r.style?.highlight != null ||
          ((r.style?.roleId ?? 0) >= 2 && !r.style?.printed),
      ),
    );

    setParagraphs(
      rows.map((row) =>
        newPara(
          cleanText(row.text),
          (row.align ?? 0) as 0 | 1 | 2,
          row.indentEm ?? 0,
          row.runs?.map((r) => {
            const runStyle = r.style ? { ...r.style } : undefined;
            if (hasHighlights) {
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
              // 纯手写模式：所有 run 清理为 role_id = 0, printed = false, fontPath = undefined
              if (runStyle) {
                runStyle.roleId = 0;
                runStyle.printed = false;
                runStyle.fontPath = undefined;
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

    // 自动匹配文档主字体 -> Role 1 (打印体)
    if (res.docFontFamily) {
      const matched = matchSystemFont(res.docFontFamily);
      if (matched) {
        role1.fontPath = matched.path;
      }
    }

    const nextRoles: UiHandwritingRole[] = [role0, role1];

    if (hasHighlights) {
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

    store.roles = nextRoles;

    focusPara(store.paragraphs[0].id, 0);
    store.status = `已导入 ${rows.length} 个段落，回车分段、按钮设格式`;
    scheduleRender();
  } catch (e) {
    store.status = `导入 docx 失败：${e}`;
  }
}

export async function importDocument(): Promise<void> {
  const p = await dialogs.pickDocument();
  if (!p) return;
  store.status = "正在渲染文档底图…";
  try {
    const res = await api.importDocument(p);
    if (!res.pages.length) throw new Error("未得到任何页面");
    store.docPages = res.pages; // 先写 docPages 再切背景，避免 syncDocState 误清
    store.docStatus = `已导入 ${res.pages.length} 页，可逐页框选`;
    store.backgroundPath = res.pages[0];
    loadBgDimensions(res.pages[0]);
    if (res.regions.length > 0) {
      store.regions = res.regions;
      store.selectedRegionIndex = 0;
      store.status = `已导入文档底图（共 ${res.pages.length} 页），并自动识别提取了 ${res.regions.length} 处手写填空区域！`;
    } else {
      store.status = `已导入文档底图（共 ${res.pages.length} 页）`;
    }
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

export function applyPreset(p: UiParams, msg?: string): void {
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
    store.roles = p.roles.map((r) => ({ ...r }));
  }
  store.docPages = null;
  store.docStatus = "";
  if (msg) store.status = msg;
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
    const p = await api.loadPreset(path);
    applyPreset(p, `已载入预设：${path}`);
    scheduleRender();
  } catch (e) {
    store.status = `载入失败：${e}`;
  }
}

export async function loadPresetFromDialog(): Promise<void> {
  const p = await dialogs.pickPreset();
  if (!p) return;
  await selectPreset(p);
  store.status = "预设已载入（含边距/扰动参数）";
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
  } catch (e) {
    store.status = `导出失败：${e}`;
  }
}

export async function exportPdf(): Promise<void> {
  const path = await dialogs.savePdf();
  if (typeof path !== "string") return;
  store.status = "导出中…";
  try {
    await api.exportPdf(buildParams(), path);
    store.status = `PDF 已导出：${path}`;
  } catch (e) {
    store.status = `导出 PDF 失败：${e}`;
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

/** 区域列表摘要（对齐 models.rs TextRegion::label；自定义过追加 ⚙ 标记）。 */
export function regionLabel(r: Region, index: number): string {
  const style = r.printed ? "打印" : "手写";
  const page = r.page > 1 ? ` 第${r.page}页` : "";
  const custom = regionHasOverrides(r) ? " ⚙" : "";
  return `${index}. ${style}${page} ${[...r.text].length}字 (${r.x},${r.y} ${r.w}×${r.h})${custom}`;
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
  if (on) store.editingIndex = -1;
}

/** 区域对话框草稿默认值。 */
const defaultRegionDraft = (page: number): Region => ({
  x: 0,
  y: 0,
  w: 0,
  h: 0,
  text: "",
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

    const hasHighlights = rows.some((row) =>
      row.runs?.some(
        (r) =>
          r.style?.highlight != null ||
          ((r.style?.roleId ?? 0) >= 2 && !r.style?.printed),
      ),
    );

    d.paragraphs = rows.map((row) => ({
      text: cleanText(row.text),
      align: (row.align ?? 0) as 0 | 1 | 2,
      indentEm: row.indentEm ?? 0,
      runs: row.runs?.map((r) => {
        const runStyle = r.style ? { ...r.style } : undefined;
        if (hasHighlights) {
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
  scheduleRender();
}

// ---------------------------------------------------------------- 关于与更新

export function openAboutModal(): void {
  store.aboutModalOpen = true;
}

export function closeAboutModal(): void {
  store.aboutModalOpen = false;
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

/** 软件启动时静默检查更新 */
export async function startupCheckUpdate(): Promise<void> {
  if (!store.autoCheckUpdate) return;

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


