<script setup lang="ts">
//! 预览区：contain-fit 显示当前页 + 框选文字区域（交互对齐 Python 版 PreviewLabel）。
//!
//! 与 Python 版一致的行为：
//! - 「框选文字」为翻页导航行的勾选式按钮；勾选后十字光标，拖出矩形松开弹对话框
//! - 已有区域**不常驻画框**：仅悬浮右侧列表项时临时显示红色虚线框 + 浅填充，
//!   且仅当区域所在页 == 当前页、且不处于调整态
//! - 单击列表项进入调整态：橡皮筋编辑框可整体移动 / 八向缩放（无手柄，8px 容差）
//! - 命中优先级：编辑框命中 > 新建框选（即使框选模式开启）
//! - Esc 或点击编辑框外空白退出调整态；调整几何未变化时不写回
//! - 新建/调整阈值：拖动 <4 显示像素视为误触；resize 增量 <4 显示像素忽略
//!
//! 坐标链：区域存背景原始像素 → ×factor(=drawW/bgNatW) → 显示像素。

import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { NButton, NTooltip } from "naive-ui";
import {
  clampRect,
  cancelRegionEdit,
  nextPage,
  openEditRegionDialog,
  openNewRegionDialog,
  prevPage,
  setRegionMode,
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

/** 悬浮列表项的临时虚线高亮（同页且非调整态才显示，对齐 _on_region_hover） */
const highlightBox = computed(() => {
  const i = store.highlightIndex;
  if (i < 0 || i >= store.regions.length) return null;
  if (store.editingIndex >= 0) return null; // 调整态下不叠加悬浮高亮
  const r = store.regions[i];
  if (r.page - 1 !== store.pageIndex) return null;
  const f = factor.value;
  return { x: r.x * f, y: r.y * f, w: Math.max(1, r.w * f), h: Math.max(1, r.h * f) };
});

// ---- 交互状态 ----
type Zone = "outside" | "tl" | "tr" | "bl" | "br" | "l" | "r" | "t" | "b" | "move";
const adjusting = ref(false);
const zone = ref<Zone>("outside");
const press = reactive({ x: 0, y: 0 });
const hover = reactive({ x: -99, y: -99 });
/** 橡皮筋实时几何（新建与编辑共用，显示像素） */
const rb = reactive({ x: 0, y: 0, w: 0, h: 0 });

const hasEdit = computed(
  () => store.editingIndex >= 0 && store.editingIndex < store.regions.length,
);

/** 编辑框基准几何（显示像素）；交互中取 rb */
const ebBase = computed(() => {
  if (!hasEdit.value) return null;
  const r = store.regions[store.editingIndex];
  const f = factor.value;
  return { x: r.x * f, y: r.y * f, w: r.w * f, h: r.h * f };
});
const editBox = computed(() => (adjusting.value ? { ...rb } : ebBase.value));

function cur(): { x: number; y: number; w: number; h: number } {
  return editBox.value ?? { x: 0, y: 0, w: 0, h: 0 };
}

/** 命中部位判定（8px 容差，对齐 _hit_zone） */
function zoneAt(px: number, py: number): Zone {
  if (!hasEdit.value) return "outside";
  const e = cur();
  const m = 8;
  const nearL = Math.abs(px - e.x) <= m;
  const nearR = Math.abs(px - (e.x + e.w)) <= m;
  const nearT = Math.abs(py - e.y) <= m;
  const nearB = Math.abs(py - (e.y + e.h)) <= m;
  const inV = py >= e.y && py <= e.y + e.h;
  const inH = px >= e.x && px <= e.x + e.w;
  if (nearL && nearT) return "tl";
  if (nearR && nearT) return "tr";
  if (nearL && nearB) return "bl";
  if (nearR && nearB) return "br";
  if (nearL && inV) return "l";
  if (nearR && inV) return "r";
  if (nearT && inH) return "t";
  if (nearB && inH) return "b";
  if (inH && inV) return "move";
  return "outside";
}

const ZONE_CURSORS: Record<string, string> = {
  tl: "nwse-resize",
  br: "nwse-resize",
  tr: "nesw-resize",
  bl: "nesw-resize",
  l: "ew-resize",
  r: "ew-resize",
  t: "ns-resize",
  b: "ns-resize",
  move: "move",
};

const cursorStyle = computed(() => {
  if (adjusting.value) return ZONE_CURSORS[zone.value] ?? "default";
  if (hasPreview.value && hasEdit.value) {
    const z = zoneAt(hover.x, hover.y);
    if (z !== "outside") return ZONE_CURSORS[z];
  }
  if (store.regionMode) return "crosshair";
  return "default";
});

// ---- 指针事件 ----
function localPoint(e: { clientX: number; clientY: number }): { x: number; y: number } {
  const rect = interactEl.value?.getBoundingClientRect();
  return rect ? { x: e.clientX - rect.left, y: e.clientY - rect.top } : { x: 0, y: 0 };
}
function clampPt(p: { x: number; y: number }): { x: number; y: number } {
  // 钳制在图纸（draw-area）范围内，对齐 _apply_adjust 的 area 钳制
  return {
    x: Math.max(0, Math.min(p.x, draw.value.w)),
    y: Math.max(0, Math.min(p.y, draw.value.h)),
  };
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

function beginAdjust(z: Zone, p: { x: number; y: number }): void {
  adjusting.value = true;
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

function onDown(e: PointerEvent): void {
  if (e.button !== 0 || !hasPreview.value) return;
  e.preventDefault();
  void interactEl.value?.setPointerCapture(e.pointerId);
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;

  // 已有编辑框优先命中：抓取移动/缩放（即使处于新建模式，对齐 mousePressEvent）
  if (hasEdit.value) {
    const z = zoneAt(p.x, p.y);
    if (z !== "outside") {
      beginAdjust(z, p);
      return;
    }
  }
  if (store.regionMode) {
    // 新建框选拖拽；若处于编辑态先退出（对齐 finish_edit(emit=True)）
    if (hasEdit.value) cancelRegionEdit();
    adjusting.value = false;
    zone.value = "outside";
    press.x = p.x;
    press.y = p.y;
    rb.x = p.x;
    rb.y = p.y;
    rb.w = 0;
    rb.h = 0;
    rubberLive.value = true;
    return;
  }
  if (hasEdit.value) {
    // 非新建模式下点击框外：结束调整（emit=True）
    cancelRegionEdit();
  }
}

/** 新建橡皮筋是否可见（区别于编辑框复用同一几何容器） */
const rubberLive = ref(false);

function applyAdjust(p: { x: number; y: number }): void {
  const cp = clampPt(p);
  const z = zone.value;
  if (z === "move") {
    // 整体平移：钳制在图纸范围内
    const dx = cp.x - press.x;
    const dy = cp.y - press.y;
    rb.x = Math.max(0, Math.min(rb.x + dx, draw.value.w - rb.w));
    rb.y = Math.max(0, Math.min(rb.y + dy, draw.value.h - rb.h));
    press.x = cp.x;
    press.y = cp.y;
    return;
  }
  let l = rb.x;
  let t = rb.y;
  let r = rb.x + rb.w;
  let b = rb.y + rb.h;
  if (z.includes("l")) l = cp.x;
  if (z.includes("r")) r = cp.x;
  if (z.includes("t")) t = cp.y;
  if (z.includes("b")) b = cp.y;
  // 过小的增量直接忽略，保持上一几何（对齐 new.width() >= 4）
  const nx = Math.min(l, r);
  const ny = Math.min(t, b);
  const nw = Math.abs(r - l);
  const nh = Math.abs(b - t);
  if (nw >= 4 && nh >= 4) {
    rb.x = nx;
    rb.y = ny;
    rb.w = nw;
    rb.h = nh;
  }
}

function onMove(e: PointerEvent): void {
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;
  if (adjusting.value) {
    applyAdjust(p);
    return;
  }
  if (rubberLive.value && store.regionMode) {
    rb.x = Math.min(press.x, p.x);
    rb.y = Math.min(press.y, p.y);
    rb.w = Math.abs(p.x - press.x);
    rb.h = Math.abs(p.y - press.y);
  }
}

function onUp(e: PointerEvent): void {
  const p = localPoint(e);
  if (adjusting.value) {
    const changed =
      !ebBase.value ||
      Math.round(rb.x) !== Math.round(ebBase.value.x) ||
      Math.round(rb.y) !== Math.round(ebBase.value.y) ||
      Math.round(rb.w) !== Math.round(ebBase.value.w) ||
      Math.round(rb.h) !== Math.round(ebBase.value.h);
    adjusting.value = false;
    zone.value = "outside";
    hover.x = p.x;
    hover.y = p.y;
    // 几何真的变了才写回（对齐 rect != press_rect 判断）
    if (changed && store.editingIndex >= 0) {
      const rect = finishRect({ ...rb });
      if (rect) updateRegionGeometry(store.editingIndex, rect);
    }
    return;
  }
  if (rubberLive.value && store.regionMode) {
    rubberLive.value = false;
    // 过滤误触：按控件坐标判断拖动距离（阈值 4 显示像素）
    if (rb.w < 4 || rb.h < 4) return;
    const rect = finishRect({ ...rb });
    if (rect) openNewRegionDialog(rect);
  }
}

function onCancel(): void {
  adjusting.value = false;
  zone.value = "outside";
  rubberLive.value = false;
}

/** 双击区域 → 打开属性对话框（修改文字 / 参数）。命中当前页最上层包含点的区域。 */
function onDblClick(e: MouseEvent): void {
  if (!hasPreview.value || store.dialogOpen) return;
  const p = localPoint(e);
  const f = factor.value;
  for (let i = store.regions.length - 1; i >= 0; i--) {
    const r = store.regions[i];
    if (r.page - 1 !== store.pageIndex) continue;
    const x = r.x * f;
    const y = r.y * f;
    if (p.x >= x && p.x <= x + r.w * f && p.y >= y && p.y <= y + r.h * f) {
      openEditRegionDialog(i);
      return;
    }
  }
}

const boxBackground = computed(() =>
  store.previewBgIdx % 2 === 0 ? "#c8d0ca" : "#565b56",
);

/** 编辑框手柄圆点位置：四角 + 四边中点（纯视觉提示，命中仍靠 8px 容差） */
const HANDLE_DOTS: Record<string, string>[] = [
  { left: "-4px", top: "-4px" },
  { left: "calc(50% - 3.5px)", top: "-4px" },
  { right: "-4px", top: "-4px" },
  { right: "-4px", top: "calc(50% - 3.5px)" },
  { right: "-4px", bottom: "-4px" },
  { left: "calc(50% - 3.5px)", bottom: "-4px" },
  { left: "-4px", bottom: "-4px" },
  { left: "-4px", top: "calc(50% - 3.5px)" },
];

function pxStyle(r: { x: number; y: number; w: number; h: number }) {
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

          <!-- 悬浮列表项的临时高亮：红虚线框 + 浅填充（对齐 _draw_rect_overlays） -->
          <div v-if="highlightBox" class="region-highlight" :style="pxStyle(highlightBox)" />

          <!-- 调整态橡皮筋编辑框（新建拖拽共用样式，无手柄） -->
          <div
            v-if="(adjusting || rubberLive) && (rb.w > 0 || rb.h > 0)"
            class="rubber-band"
            :style="pxStyle(rb)"
          />
          <!-- 非交互中的编辑框（进入调整态后静止显示）：四角 + 四边中点手柄提示可拖拽/缩放 -->
          <div v-else-if="hasEdit && editBox" class="rubber-band is-editing" :style="pxStyle(editBox)">
            <span
              v-for="(h, i) in HANDLE_DOTS"
              :key="i"
              class="handle-dot"
              :style="h"
            />
          </div>

          <div
            ref="interactEl"
            class="interact-layer"
            :style="{ cursor: cursorStyle }"
            @pointerdown="onDown"
            @pointermove="onMove"
            @pointerup="onUp"
            @pointercancel="onCancel"
            @dblclick="onDblClick"
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
      <NTooltip trigger="hover" placement="top">
        <template #trigger>
          <NButton
            size="small"
            :type="store.regionMode ? 'primary' : 'default'"
            :ghost="store.regionMode"
            @click="setRegionMode(!store.regionMode)"
          >
            框选文字
          </NButton>
        </template>
        勾选后在预览图上按住鼠标拖出矩形，松开后输入该区域的文字<br />
        （可独立选择手写体 / 打印体，实现混排）
      </NTooltip>
    </div>
  </div>
</template>
