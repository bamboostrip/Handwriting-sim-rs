//! 预览视口几何：contain-fit、缩放、平移、聚焦区域的纯函数。
//!
//! 坐标约定：
//! - 底图坐标（`Rect`）与归一化中心（`cx/cy`）以**背景原始像素**为准，与区域存储一致；
//! - `drawRect` 返回**容器 CSS 像素**；缩放只改变显示尺寸，绝不改写区域坐标。
//!
//! 视口模型：
//! - `fit` 模式：始终等于「适应窗口」比例，窗口尺寸变化自动跟随；
//! - `manual` 模式：保留用户选择的绝对比例（CSS 像素 / 底图像素），
//!   窗口变大时不会自动缩回，只做下限与边界钳制。
//!
//! 这些函数不依赖 Vue / DOM，便于单测（node 直接跑 .ts 即可）。

/** 手动缩放上限：400%（超过后只是插值放大，不再有新细节）。 */
export const MAX_ABS_SCALE = 4;
/** 「聚焦区域」上限：200%，避免小格子被放到过度模糊。 */
export const FOCUS_MAX_SCALE = 2;

export type ViewMode = "fit" | "manual";

export interface ViewState {
  mode: ViewMode;
  /** manual 模式：每个底图像素对应的 CSS 像素数（fit 模式下忽略，由容器实时算出） */
  scale: number;
  /** 画面中心在底图中的归一化位置（0..1）；仅 manual 模式有意义 */
  cx: number;
  cy: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 新建「适应窗口」视口。 */
export function makeFitView(): ViewState {
  return { mode: "fit", scale: 0, cx: 0.5, cy: 0.5 };
}

/** contain-fit 比例：整页完整可见时的显示比例。 */
export function fitScale(
  natW: number,
  natH: number,
  cw: number,
  ch: number,
): number {
  if (!(natW > 0) || !(natH > 0) || !(cw > 0) || !(ch > 0)) return 0;
  return Math.min(cw / natW, ch / natH);
}

/** 当前实际显示比例（CSS 像素 / 底图像素）。 */
export function effectiveScale(view: ViewState, fit: number): number {
  if (view.mode === "fit") return fit;
  return clampNumber(view.scale, Math.min(fit, MAX_ABS_SCALE), MAX_ABS_SCALE);
}

/** 按视口状态算出图纸在容器中的矩形（已居中 / 钳制，不会把图纸拖出视野）。 */
export function drawRect(
  view: ViewState,
  fit: number,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
): Rect {
  const scale = effectiveScale(view, fit);
  const dw = natW * scale;
  const dh = natH * scale;
  const center = clampCenter(view, scale, natW, natH, cw, ch);
  return {
    x: cw / 2 - center.cx * dw,
    y: ch / 2 - center.cy * dh,
    w: dw,
    h: dh,
  };

}

/** 钳制视口：比例不小于当前适应比例，中心不越界。 */
export function clampView(
  view: ViewState,
  fit: number,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
): ViewState {
  const low = Math.min(fit, MAX_ABS_SCALE);
  const scale = clampNumber(effectiveScale(view, fit), low, MAX_ABS_SCALE);
  const center = clampCenter(view, scale, natW, natH, cw, ch);
  return { mode: view.mode, scale, cx: center.cx, cy: center.cy };
}

/**
 * 以容器内一点 `(px, py)` 为锚点缩放：缩放前后该点对应的底图像素保持不动。
 * 目标比例低于适应窗口时直接回到 fit 模式。
 */
export function zoomAt(
  view: ViewState,
  fit: number,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
  px: number,
  py: number,
  nextScale: number,
): ViewState {
  const current = effectiveScale(view, fit);
  if (!(current > 0) || !(natW > 0) || !(natH > 0)) return view;

  const next = clampNumber(
    nextScale,
    Math.min(fit, MAX_ABS_SCALE),
    MAX_ABS_SCALE,
  );
  if (next <= fit + 1e-6) return makeFitView();
  if (Math.abs(next - current) < 1e-9) return view;

  const rect = drawRect(view, fit, natW, natH, cw, ch);
  // 光标可能在留白区：夹到图纸边缘，避免缩放瞬间跳帧
  const u = clampNumber((px - rect.x) / rect.w, 0, 1);
  const v = clampNumber((py - rect.y) / rect.h, 0, 1);

  const dw = natW * next;
  const dh = natH * next;
  const x = px - u * dw;
  const y = py - v * dh;
  const cx = (cw / 2 - x) / dw;
  const cy = (ch / 2 - y) / dh;
  return clampView({ mode: "manual", scale: next, cx, cy }, fit, natW, natH, cw, ch);
}

/**
 * 平移：`dx/dy` 是鼠标自按下以来的**累计**位移（CSS 像素），
 * 基于按下时的视口 `base` 计算，避免多次 move 累积漂移。
 * 适应窗口状态下无平移余量，原样返回。
 */
export function panBy(
  base: ViewState,
  dx: number,
  dy: number,
  fit: number,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
): ViewState {
  if (base.mode === "fit") return base;
  const scale = effectiveScale(base, fit);
  if (!(scale > 0) || !(natW > 0) || !(natH > 0)) return base;
  const cx = base.cx - dx / (natW * scale);
  const cy = base.cy - dy / (natH * scale);
  return clampView({ mode: "manual", scale, cx, cy }, fit, natW, natH, cw, ch);
}

/**
 * 把某个区域（底图像素）放到视口中央：留 `pad` 边距，比例不超过
 * {@link FOCUS_MAX_SCALE}，且不小于适应比例；区域太大时退回适应窗口。
 */
export function focusRectView(
  rect: Rect,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
  pad = 32,
): ViewState {
  const fit = fitScale(natW, natH, cw, ch);
  if (!(fit > 0) || !(natW > 0) || !(natH > 0)) return makeFitView();
  if (!(rect.w > 0) || !(rect.h > 0)) return makeFitView();

  const availW = Math.max(40, cw - pad * 2);
  const availH = Math.max(40, ch - pad * 2);
  const target = Math.min(availW / rect.w, availH / rect.h, FOCUS_MAX_SCALE);
  const scale = clampNumber(target, Math.min(fit, MAX_ABS_SCALE), MAX_ABS_SCALE);
  if (scale <= fit + 1e-6) return makeFitView();

  const cx = (rect.x + rect.w / 2) / natW;
  const cy = (rect.y + rect.h / 2) / natH;
  return clampView({ mode: "manual", scale, cx, cy }, fit, natW, natH, cw, ch);
}

/** 显示百分比（按绝对比例：适应窗口通常 25%~40%，100% = 1 底图像素 : 1 CSS 像素）。 */
export function percentOf(view: ViewState, fit: number): number {
  return Math.round(effectiveScale(view, fit) * 100);
}

// ---------------------------------------------------------------- 内部工具

function clampNumber(value: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(value, hi));
}

/**
 * 中心钳制：图纸比视口大时不允许露出底色（x ∈ [cw-dw, 0] 等价于
 * cx ∈ [cw/(2dw), 1-cw/(2dw)]）；比视口小时固定居中。
 */
function clampCenter(
  view: ViewState,
  scale: number,
  natW: number,
  natH: number,
  cw: number,
  ch: number,
): { cx: number; cy: number } {
  const dw = natW * scale;
  const dh = natH * scale;
  let cx = Number.isFinite(view.cx) ? view.cx : 0.5;
  let cy = Number.isFinite(view.cy) ? view.cy : 0.5;
  if (dw <= cw) {
    cx = 0.5;
  } else {
    cx = clampNumber(cx, cw / (2 * dw), 1 - cw / (2 * dw));
  }
  if (dh <= ch) {
    cy = 0.5;
  } else {
    cy = clampNumber(cy, ch / (2 * dh), 1 - ch / (2 * dh));
  }
  return { cx, cy };
}
