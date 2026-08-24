<script setup lang="ts">
//! 预览区：contain-fit 显示当前页 + 框选交互。
//!
//! 交互逻辑逐行移植自原 Slint 版 main_window.slint 的 TouchArea（sel）：
//! - 模式 0 空闲 / 1 橡皮筋新建框选 / 2 二次调整（8px 容差命中 move/角/边）
//! - 坐标链：区域存背景原始像素 → ×factor(=drawW/bgNatW) → 显示像素
//!   （等价于原版 fit-scale × bg-preview-scale 两级换算）
//! - 新建矩形 ≥4 显示像素才生效；调整增量 <3 显示像素忽略
//! - 点击框外空白 / Esc 退出调整态

import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { NButton } from "naive-ui";
import {
  clampRect,
  cancelRegionEdit,
  nextPage,
  openNewRegionDialog,
  prevPage,
  store,
  togglePreviewBg,
  updateRegionGeometry,
} from "../store";

// ---- 容器尺寸 ----
const boxEl = ref<HTMLDivElement | null>(null);
const interactEl = ref<HTMLDivElement | null>(null);
const cw = ref(0);
const chh = ref(0);
let ro: ResizeObserver | null = null;

onMounted(() => {
  ro = new ResizeObserver(() => {
    if (boxEl.value) {
      cw.value = boxEl.value.clientWidth;
      chh.value = boxEl.value.clientHeight;
    }
  });
  if (boxEl.value) ro.observe(boxEl.value);
});
onBeforeUnmount(() => ro?.disconnect());

// ---- contain-fit 几何 ----
const currentPage = computed(() => store.previewPages[store.pageIndex]);
const natW = computed(() => currentPage.value?.width ?? 0);
const natH = computed(() => currentPage.value?.height ?? 0);
const hasPreview = computed(
  () => store.previewPages.length > 0 && natW.value > 0 && natH.value > 0,
);

const fit = computed(() => {
  if (!hasPreview.value || cw.value <= 0 || chh.value <= 0) return 0;
  return Math.min(cw.value / natW.value, chh.value / natH.value);
});
const draw = computed(() => {
  const dw = natW.value * fit.value;
  const dh = natH.value * fit.value;
  return { x: (cw.value - dw) / 2, y: (chh.value - dh) / 2, w: dw, h: dh };
});
const drawStyle = computed(() => ({
  left: `${draw.value.x}px`,
  top: `${draw.value.y}px`,
  width: `${draw.value.w}px`,
  height: `${draw.value.h}px`,
}));

/** 背景原始像素 → 显示像素 */
const factor = computed(() =>
  store.bgNatW > 0 ? draw.value.w / store.bgNatW : 1,
);

const pageText = computed(() => {
  const total = store.previewPages.length;
  return `第 ${total === 0 ? 1 : store.pageIndex + 1} / ${total || 1} 页`;
});

// ---- 已有区域叠加（编辑中的不显示，跨页过滤）----
const visibleOverlays = computed(() =>
  store.regions
    .map((r, index) => ({ r, index }))
    .filter(
      ({ r, index }) =>
        r.page - 1 === store.pageIndex && !(index === store.editingIndex) && r.w > 0,
    ),
);
function overlayStyle(r: { x: number; y: number; w: number; h: number }) {
  const f = factor.value;
  return {
    left: `${r.x * f}px`,
    top: `${r.y * f}px`,
    width: `${Math.max(1, r.w * f)}px`,
    height: `${Math.max(1, r.h * f)}px`,
  };
}

// ---- 交互状态 ----
type Zone = "none" | "tl" | "tr" | "bl" | "br" | "l" | "r" | "t" | "b" | "move";
const mode = ref<0 | 1 | 2>(0);
const zone = ref<Zone>("none");
const press = reactive({ x: 0, y: 0 });
const hover = reactive({ x: -99, y: -99 });
const rb = reactive({ x: 0, y: 0, w: 0, h: 0 });

const hasEdit = computed(
  () => store.editingIndex >= 0 && store.editingIndex < store.regions.length,
);

/** 编辑框基准几何（显示像素；交互中取 rb） */
const ebBase = computed(() => {
  if (!hasEdit.value) return null;
  const r = store.regions[store.editingIndex];
  const f = factor.value;
  return { x: r.x * f, y: r.y * f, w: r.w * f, h: r.h * f };
});
const eb = computed(() => (mode.value !== 0 ? { ...rb } : ebBase.value));

function cur(): { x: number; y: number; w: number; h: number } {
  return eb.value ?? { x: 0, y: 0, w: 0, h: 0 };
}

/** 命中部位判定（8px 容差，对齐原 _hit_zone） */
function zoneAt(px: number, py: number): Zone {
  const e = cur();
  if (!hasEdit.value || mode.value !== 0) return mode.value !== 0 ? zone.value : "none";
  const tol = 8;
  if (Math.abs(px - e.x) <= tol && Math.abs(py - e.y) <= tol) return "tl";
  if (Math.abs(px - (e.x + e.w)) <= tol && Math.abs(py - e.y) <= tol) return "tr";
  if (Math.abs(px - e.x) <= tol && Math.abs(py - (e.y + e.h)) <= tol) return "bl";
  if (Math.abs(px - (e.x + e.w)) <= tol && Math.abs(py - (e.y + e.h)) <= tol) return "br";
  if (Math.abs(px - e.x) <= tol && py >= e.y && py <= e.y + e.h) return "l";
  if (Math.abs(px - (e.x + e.w)) <= tol && py >= e.y && py <= e.y + e.h) return "r";
  if (Math.abs(py - e.y) <= tol && px >= e.x && px <= e.x + e.w) return "t";
  if (Math.abs(py - (e.y + e.h)) <= tol && px >= e.x && px <= e.x + e.w) return "b";
  if (px >= e.x && px <= e.x + e.w && py >= e.y && py <= e.y + e.h) return "move";
  return "none";
}

function cursorFor(z: Zone): string {
  switch (z) {
    case "tl":
    case "br":
      return "nwse-resize";
    case "tr":
    case "bl":
      return "nesw-resize";
    case "l":
    case "r":
      return "ew-resize";
    case "t":
    case "b":
      return "ns-resize";
    case "move":
      return "move";
    default:
      return "crosshair";
  }
}

const cursorStyle = computed(() => {
  if (mode.value === 1) return "crosshair";
  if (mode.value === 2) return cursorFor(zone.value);
  if (store.regionMode) return "crosshair";
  if (hasEdit.value) return cursorFor(zoneAt(hover.x, hover.y));
  return "default";
});

// ---- 指针事件 ----
function localPoint(e: PointerEvent): { x: number; y: number } {
  const rect = interactEl.value?.getBoundingClientRect();
  return rect ? { x: e.clientX - rect.left, y: e.clientY - rect.top } : { x: 0, y: 0 };
}
function clampPt(p: { x: number; y: number }): { x: number; y: number } {
  return {
    x: Math.max(0, Math.min(p.x, draw.value.w)),
    y: Math.max(0, Math.min(p.y, draw.value.h)),
  };
}
/** 调整后的新边（仅对应部位跟随鼠标） */
function adjL(p: { x: number; y: number }, z: Zone): number {
  return z === "l" || z === "tl" || z === "bl" ? clampPt(p).x : rb.x;
}
function adjT(p: { x: number; y: number }, z: Zone): number {
  return z === "t" || z === "tl" || z === "tr" ? clampPt(p).y : rb.y;
}
function adjR(p: { x: number; y: number }, z: Zone): number {
  return z === "r" || z === "tr" || z === "br" ? clampPt(p).x : rb.x + rb.w;
}
function adjB(p: { x: number; y: number }, z: Zone): number {
  return z === "b" || z === "bl" || z === "br" ? clampPt(p).y : rb.y + rb.h;
}
function toSrcRect(r: { x: number; y: number; w: number; h: number }) {
  const f = factor.value;
  return { x: r.x / f, y: r.y / f, w: r.w / f, h: r.h / f };
}
function finishRect(display: {
  x: number;
  y: number;
  w: number;
  h: number;
}): [number, number, number, number] | null {
  const s = toSrcRect(display);
  return clampRect(s.x, s.y, s.w, s.h, store.bgNatW, store.bgNatH);
}

function onDown(e: PointerEvent): void {
  if (e.button !== 0 || !hasPreview.value) return;
  e.preventDefault();
  void interactEl.value?.setPointerCapture(e.pointerId);
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;
  if (hasEdit.value && mode.value === 0 && zoneAt(p.x, p.y) === "none") {
    // 点击框外空白：退出调整态（对齐 Python 版点击外部取消）
    cancelRegionEdit();
  }
  if (store.regionMode) {
    mode.value = 1;
    press.x = p.x;
    press.y = p.y;
    rb.x = p.x;
    rb.y = p.y;
    rb.w = 0;
    rb.h = 0;
  } else if (hasEdit.value && mode.value === 0) {
    const z = zoneAt(p.x, p.y);
    if (z !== "none") {
      mode.value = 2;
      zone.value = z;
      const cp = clampPt(p);
      press.x = cp.x;
      press.y = cp.y;
      const base = cur();
      rb.x = base.x;
      rb.y = base.y;
      rb.w = base.w;
      rb.h = base.h;
    }
  }
}

function onMove(e: PointerEvent): void {
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;
  if (mode.value === 1) {
    rb.x = Math.min(press.x, p.x);
    rb.y = Math.min(press.y, p.y);
    rb.w = Math.abs(p.x - press.x);
    rb.h = Math.abs(p.y - press.y);
  } else if (mode.value === 2) {
    const z = zone.value;
    if (z === "move") {
      // 整体移动：钳制在显示图范围内
      const cp = clampPt(p);
      const dx = cp.x - press.x;
      const dy = cp.y - press.y;
      rb.x = Math.max(0, Math.min(rb.x + dx, draw.value.w - rb.w));
      rb.y = Math.max(0, Math.min(rb.y + dy, draw.value.h - rb.h));
      press.x = cp.x;
      press.y = cp.y;
    } else {
      // 边/角缩放：过小增量忽略，保持上一几何（≥3 显示像素）
      const l = adjL(p, z);
      const t = adjT(p, z);
      const r = adjR(p, z);
      const b = adjB(p, z);
      if (Math.abs(r - l) >= 3 && Math.abs(b - t) >= 3) {
        rb.x = Math.min(l, r);
        rb.y = Math.min(t, b);
        rb.w = Math.abs(r - l);
        rb.h = Math.abs(b - t);
      }
    }
  }
}

function onUp(e: PointerEvent): void {
  if (mode.value === 1) {
    mode.value = 0;
    // 过滤误触的极小选区（≥4 显示像素，对齐 Python 版）
    if (rb.w >= 4 && rb.h >= 4) {
      const rect = finishRect({ ...rb });
      if (rect) openNewRegionDialog(rect);
    }
  } else if (mode.value === 2) {
    const p = clampPt(localPoint(e));
    const z = zone.value;
    const l = adjL(p, z);
    const t = adjT(p, z);
    const r = adjR(p, z);
    const b = adjB(p, z);
    mode.value = 0;
    zone.value = "none";
    const rect = finishRect({
      x: Math.min(l, r),
      y: Math.min(t, b),
      w: Math.abs(r - l),
      h: Math.abs(b - t),
    });
    if (rect && store.editingIndex >= 0) updateRegionGeometry(store.editingIndex, rect);
  }
}

function onCancel(): void {
  mode.value = 0;
  zone.value = "none";
}

const rubberStyle = computed(() => ({
  left: `${rb.x}px`,
  top: `${rb.y}px`,
  width: `${rb.w}px`,
  height: `${rb.h}px`,
}));

const boxBackground = computed(() =>
  store.previewBgIdx % 2 === 0 ? "#c8d0ca" : "#565b56",
);

function rectStyle(r: { x: number; y: number; w: number; h: number }) {
  return {
    left: `${r.x}px`,
    top: `${r.y}px`,
    width: `${Math.max(1, r.w)}px`,
    height: `${Math.max(1, r.h)}px`,
  };
}
</script>

<template>
  <div class="preview-col">
    <div ref="boxEl" class="preview-box" :style="{ background: boxBackground }">
      <template v-if="hasPreview">
        <div class="draw-area" :style="drawStyle">
          <img class="page-img" :src="currentPage.url" alt="" />
          <div
            v-for="vo in visibleOverlays"
            :key="vo.index"
            class="region-overlay"
            :class="{ 'is-highlight': vo.index === store.highlightIndex }"
            :style="overlayStyle(vo.r)"
          />
          <div v-if="mode === 1" class="rubber-band" :style="rubberStyle" />
          <div v-if="hasEdit && eb" class="edit-box" :style="rectStyle(eb)">
            <span class="handle" style="left: -4px; top: -4px"></span>
            <span class="handle" style="right: -4px; top: -4px"></span>
            <span class="handle" style="left: -4px; bottom: -4px"></span>
            <span class="handle" style="right: -4px; bottom: -4px"></span>
          </div>
          <div
            ref="interactEl"
            class="interact-layer"
            :style="{ cursor: cursorStyle }"
            @pointerdown="onDown"
            @pointermove="onMove"
            @pointerup="onUp"
            @pointercancel="onCancel"
          />
        </div>
      </template>
      <div v-else class="empty-hint">选择字体与背景后点击「预览」开始生成手写体</div>
    </div>

    <div class="pagenav">
      <NButton size="small" @click="prevPage">◀ 上一页</NButton>
      <span class="page-text">{{ pageText }}</span>
      <NButton size="small" @click="nextPage">下一页 ▶</NButton>
      <NButton size="small" @click="togglePreviewBg">预览底色</NButton>
    </div>
  </div>
</template>
