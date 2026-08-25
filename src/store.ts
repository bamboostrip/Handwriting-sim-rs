//! 全局应用状态（reactive 单例）——对应原 Slint 版 main.rs 的全部 UI 逻辑：
//! 参数收集（buildParams ↔ collect_params）、防抖预览、翻页、预设、
//! 段落结构操作（分段/合并/对齐/缩进）、框选区域增删改、文档底图同步。
//!
//! 渲染代次守卫：renderSeq 只采纳最新一次请求结果，过期响应直接丢弃。

import { reactive, watch } from "vue";
import { createDiscreteApi } from "naive-ui";
import { useDebounceFn } from "@vueuse/core";
import { api, assetUrl, dialogs } from "./api";
import type { UiParams, UiRegion } from "./types";

/** 组件树外的离散弹窗（store 层的阻断性错误提示用） */
const { dialog: appDialog } = createDiscreteApi(["dialog"]);

// ---------------------------------------------------------------- 数据模型

export interface Para {
  id: number;
  text: string;
  align: 0 | 1 | 2;
  indentEm: number;
}

export type Region = UiRegion;

let paraSeq = 1;
export const newPara = (text = "", align: 0 | 1 | 2 = 0, indentEm = 0): Para => ({
  id: paraSeq++,
  text,
  align,
  indentEm,
});

/** 清理外来文本特殊字符（对齐后端 clean_text / 原 to_ui_spaces） */
export const cleanText = (s: string): string =>
  s.replace(/\u2060/g, "").replace(/[\u00a0\uffa0]/g, " ");

const PREVIEW_BG_COLORS = ["#c8d0ca", "#565b56"];

export const store = reactive({
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
});

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
    .filter((r) => cleanText(r.text).trim() !== "")
    .map((r) => ({ text: cleanText(r.text), align: r.align, indentEm: r.indentEm }));
  const hasFormat = store.paragraphs.some((r) => r.align !== 0 || r.indentEm !== 0);

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
    const rows = await api.importDocx(p, num(store.fontSize, 36));
    const fs = num(store.fontSize, 36);
    setParagraphs(
      rows.map(([t, align, indentPx]) => newPara(cleanText(t), align as 0 | 1 | 2, fs > 0 ? indentPx / fs : 0)),
    );
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
    const pages = await api.importDocument(p);
    if (!pages.length) throw new Error("未得到任何页面");
    store.docPages = pages; // 先写 docPages 再切背景，避免 syncDocState 误清
    store.docStatus = `已导入 ${pages.length} 页，可逐页框选`;
    store.backgroundPath = pages[0];
    loadBgDimensions(pages[0]);
    store.status = `已导入文档底图（${pages.length} 页）；在目标页开启「框选」即可填写`;
    scheduleRender();
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

export function setAlign(align: number): void {
  const row = store.paragraphs[curParaIndex()];
  if (!row) return;
  row.align = (Math.max(0, Math.min(2, align)) as 0 | 1 | 2);
  updateParaStatus();
  scheduleRender();
}

export function toggleIndent(on: boolean): void {
  const row = store.paragraphs[curParaIndex()];
  if (!row) return;
  row.indentEm = on ? 2.0 : 0.0;
  updateParaStatus();
  scheduleRender();
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
    r.fill != null
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

/** 新框选完成 → 立即创建空区域并选中，配置在右侧「文字区域」卡片内联完成。 */
export function createRegionFromRect(rect: [number, number, number, number]): void {
  store.regions.push({
    x: rect[0],
    y: rect[1],
    w: rect[2],
    h: rect[3],
    text: "",
    fontPath: "",
    printed: false,
    fontSize: 0,
    page: store.pageIndex + 1,
    align: 0,
    indentEm: 0,
  });
  const idx = store.regions.length - 1;
  store.selectedRegionIndex = idx;
  store.editingIndex = idx;
  store.status = `已创建区域 ${idx + 1}：在右侧「文字区域」卡片中输入文字与配置`;
}

export async function chooseRegionFont(index: number): Promise<void> {
  const r = store.regions[index];
  if (!r) return;
  const p = await dialogs.pickFont();
  if (typeof p === "string") r.fontPath = p;
}

/** 当前活动区域的有效字号（区域覆盖 > 主设置），供缩进换算与 docx 导入。 */
export function activeRegionFontSize(r: Region): number {
  return r.fontSize > 0 ? r.fontSize : num(store.fontSize, 36);
}

/** 区域对齐：作用于区域整体（0 左 / 1 中 / 2 右）。 */
export function setRegionAlign(index: number, align: number): void {
  const r = store.regions[index];
  if (!r) return;
  r.align = Math.max(0, Math.min(2, align));
  scheduleRender();
}

/** 区域首行缩进开关（2 字符，随主面板语义）。 */
export function toggleRegionIndent(index: number, on: boolean): void {
  const r = store.regions[index];
  if (!r) return;
  r.indentEm = on ? 2.0 : 0.0;
  scheduleRender();
}

/** 导入 docx 到区域：拼接全部段落文本，对齐取第一段的非左对齐值。 */
export async function importDocxToRegion(index: number): Promise<void> {
  const r = store.regions[index];
  if (!r) return;
  const p = await dialogs.pickDocx();
  if (!p) return;
  try {
    const rows = await api.importDocx(p, activeRegionFontSize(r));
    if (!rows.length) throw new Error("文档为空");
    r.text = rows.map(([t]) => cleanText(t)).join("\n");
    const aligned = rows.find(([, align]) => align !== 0);
    r.align = aligned ? (aligned[1] as 0 | 1 | 2) : 0;
    store.status = `已导入 ${rows.length} 个段落到区域 ${index + 1}`;
    scheduleRender();
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
