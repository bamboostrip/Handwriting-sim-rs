<script setup lang="ts">
//! 预览区：contain-fit / 缩放平移显示当前页 + 框选文字区域（交互对齐 Python 版 PreviewLabel）。
//!
//! 缩放与平移：
//! - 滚轮 / 触控板（Ctrl+滚轮同路径）以光标为中心缩放，范围 [适应窗口, 400%]
//! - 中键拖拽，或非框选 / 非调整态时在图纸空白处左键拖拽平移（3px 阈值，单击语义不变）
//! - 右下角缩放条：− / 适应 / 100% / +，百分比按绝对比例显示
//! - 换页 / 换底图 / 底图尺寸变化 → 回到适应窗口；同页自动重渲染保留当前视口
//! - 文字区域列表「🔍」经 store.previewFocusToken 请求聚焦到该区域
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
//! 图形标注（issue #6，与「框选文字」互斥）：
//! - 「✎ 标注」进入后十字光标拖拽起稿：虚线框实时预览，线/箭头额外用 SVG 画预览线 + 箭头头；
//!   松手把显示坐标 /factor 换回背景原始像素调 addShapeFromRect；线/箭头传起止点 points 保留方向；
//!   显示位移 <4px 视为误触不创建
//! - 非标注模式下指针优先级：已选图形 8 向手柄/内部 > shapeAtPoint 命中图形 > 原有区域逻辑 > 平移；
//!   拖内部整体移动、拖手柄缩放，松手才 updateShapeGeometry（显示增量 <4px 忽略）
//! - 选中图形显示蓝色虚线包围框 + 8 个白色手柄；点空白 / Esc 清选中。
//!   新增分支全部以 shapeMode / 图形选中为前置条件，区域框选与缩放平移路径零回归。
//!
//! 坐标链：区域存背景原始像素 → ×factor(=drawW/bgNatW) → 显示像素。
//! 缩放/平移只改变 drawW（即 factor）与图纸位置，区域坐标语义与写回逻辑完全不变。

import { computed, h, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import type { VNodeChild } from "vue";
import { NButton, NSelect, NSwitch, NTooltip } from "naive-ui";
import QuickStart from "./QuickStart.vue";
import ErrorBanner from "./ErrorBanner.vue";
import {
  MAX_ABS_SCALE,
  clampView,
  drawRect,
  effectiveScale,
  fitScale,
  focusRectView,
  makeFitView,
  panBy,
  percentOf,
  zoomAt,
  type ViewState,
} from "../viewport";
import {
  FILLABLE_SHAPE_KINDS,
  LINE_LIKE_SHAPE_KINDS,
  SHAPE_KIND_LABELS,
  SHAPE_TOOL_SOLID_TRIANGLE,
  addShapeFromRect,
  cancelRegionEdit,
  clampRect,
  clearShapeSelection,
  doRender,
  hasContent,
  jumpToRegion,
  nextPage,
  openEditRegionDialog,
  openNewRegionDialog,
  prevPage,
  selectShape,
  setRegionMode,
  setShapeDefault,
  setShapeEndpoints,
  setShapeKind,
  setShapeMode,
  shapeAtPoint,
  store,
  togglePreviewBg,
  updateRegionGeometry,
  updateShapeGeometry,
} from "../store";
import type { UiShapeKind } from "../types";

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

// ---- 当前页与视口几何 ----
const currentPage = computed(() => store.previewPages[store.pageIndex]);
const natW = computed(() => currentPage.value?.width ?? 0);
const natH = computed(() => currentPage.value?.height ?? 0);
const hasPreview = computed(
  () => store.previewPages.length > 0 && natW.value > 0 && natH.value > 0,
);

/** contain-fit 比例（「适应窗口」时的显示比例） */
const fit = computed(() => fitScale(natW.value, natH.value, cw.value, chh.value));
/** 视口状态：fit 模式跟随窗口，manual 模式保留用户缩放 */
const view = ref<ViewState>(makeFitView());
/** 当前实际显示比例（CSS 像素 / 底图像素） */
const scale = computed(() => effectiveScale(view.value, fit.value));
const draw = computed(() =>
  drawRect(view.value, fit.value, natW.value, natH.value, cw.value, chh.value),
);
const drawStyle = computed(() => ({
  left: `${draw.value.x}px`,
  top: `${draw.value.y}px`,
  width: `${draw.value.w}px`,
  height: `${draw.value.h}px`,
}));

/** 背景原始像素 → 显示像素（缩放 / 平移只改变这里） */
const factor = computed(() =>
  store.bgNatW > 0 ? draw.value.w / store.bgNatW : 1,
);

const zoomPercent = computed(() => percentOf(view.value, fit.value));
const atFit = computed(
  () => view.value.mode === "fit" || scale.value <= fit.value + 1e-6,
);
const atMax = computed(() => scale.value >= MAX_ABS_SCALE - 1e-6);
const atActual = computed(() => Math.abs(scale.value - 1) < 5e-3);
/** 图纸超出视口才有平移余量：适应窗口时中键 / 空白拖拽不生效 */
const canPan = computed(
  () => draw.value.w > cw.value + 0.5 || draw.value.h > chh.value + 0.5,
);

/** 回到「适应窗口」 */
function resetView(): void {
  view.value = makeFitView();
}
/** 窗口尺寸变化后重新钳制：manual 保留比例，但不小于新的适应比例、不越界 */
function normalizeView(): void {
  if (view.value.mode === "fit" || !hasPreview.value) return;
  view.value = clampView(
    view.value,
    fit.value,
    natW.value,
    natH.value,
    cw.value,
    chh.value,
  );
}
watch([cw, chh], normalizeView);
watch([natW, natH], resetView);
watch(() => store.pageIndex, resetView);
watch(() => store.backgroundPath, resetView);

/** 列表「🔍」请求聚焦：store 先切页再自增 token。
 *  `flush: "post"` 保证在 pageIndex / natW 的 reset watcher 之后执行（不依赖 watcher 创建顺序）。 */
watch(
  () => store.previewFocusToken,
  () => {
    const r = store.regions[store.previewFocusIndex];
    if (!r || !hasPreview.value || r.page - 1 !== store.pageIndex) return;
    // 区域坐标存的是背景原始像素；预览图可能因 >4096 降采样而更小，
    // 必须先换算到预览图像素，否则居中会按降采样比例偏移。
    const kx = store.bgNatW > 0 ? natW.value / store.bgNatW : 1;
    const ky = store.bgNatH > 0 ? natH.value / store.bgNatH : 1;
    view.value = focusRectView(
      { x: r.x * kx, y: r.y * ky, w: r.w * kx, h: r.h * ky },
      natW.value,
      natH.value,
      cw.value,
      chh.value,
      36,
    );
  },
  { flush: "post" },
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

// ---- 平移状态（累计位移基于按下时的视口，避免逐帧累积漂移） ----
const panning = ref(false);
const panStart = reactive({
  clientX: 0,
  clientY: 0,
  moved: false,
  view: makeFitView() as ViewState,
});

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

/** 8 向命中判定（8px 容差，对齐 _hit_zone）：区域调整与图形调整共用同一套几何。
 *  tl/tr/bl/br = 四角缩放，l/r/t/b = 四边缩放，move = 内部整体拖动。 */
function zoneInBox(
  e: { x: number; y: number; w: number; h: number },
  px: number,
  py: number,
): Zone {
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

/** 命中部位判定（8px 容差，对齐 _hit_zone） */
function zoneAt(px: number, py: number): Zone {
  if (!hasEdit.value) return "outside";
  return zoneInBox(cur(), px, py);
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

/** 命中当前页某个已有区域判定（返回索引，无则 -1） */
function findRegionAt(px: number, py: number): number {
  if (!hasPreview.value) return -1;
  const f = factor.value;
  for (let i = store.regions.length - 1; i >= 0; i--) {
    const r = store.regions[i];
    if (r.page - 1 !== store.pageIndex) continue;
    const x = r.x * f;
    const y = r.y * f;
    const w = r.w * f;
    const h = r.h * f;
    if (px >= x && px <= x + w && py >= y && py <= y + h) {
      return i;
    }
  }
  return -1;
}

// ---- 图形标注：显示几何 / 命中 / 选中态 ----
/** 底部下拉的可选图元（排除 polyline；顺序即下拉顺序）。
 *  `▶ 实心三角` 不是新图元 id，只是「三角形 + 实心」的一键预设：
 *  行首项目符号是图形标注里最高频的小符号，不该让用户先画三角形、
 *  再翻到右栏属性面板打开「填充」才拿到它。 */
const SHAPE_KIND_OPTIONS: { label: string; value: string }[] = (
  [
    "rect",
    "ellipse",
    "triangle",
    SHAPE_TOOL_SOLID_TRIANGLE,
    "line",
    "arrow",
    "brace_left",
    "brace_right",
    "bracket_left",
    "bracket_right",
    "check",
    "underline",
  ] as const
).map((k) =>
  k === SHAPE_TOOL_SOLID_TRIANGLE
    ? { label: "▶ 实心三角", value: SHAPE_TOOL_SOLID_TRIANGLE }
    : { label: SHAPE_KIND_LABELS[k as UiShapeKind], value: k },
);

/** 下拉当前值：选了「▶ 实心三角」时回显它，而不是裸的「三角形」。 */
const shapeToolValue = computed<string>(() =>
  store.shapeKind === "triangle" && store.shapeDefault.filled
    ? SHAPE_TOOL_SOLID_TRIANGLE
    : store.shapeKind,
);

/** 当前图元的完整文案（触发器很窄时会省略号截断，用它给 hover 兜底）。 */
const shapeToolLabel = computed<string>(
  () => SHAPE_KIND_OPTIONS.find((o) => o.value === shapeToolValue.value)?.label ?? "图元",
);

/** 下拉项自定义渲染：文本外面套一层 `title`，窄屏 / 放大字号把文字截断时，
 *  鼠标悬停仍能看到完整图元名（naive-ui 的选项内容默认 `text-overflow: ellipsis`）。 */
function renderShapeOptionLabel(option: { label?: unknown; value?: unknown }): VNodeChild {
  const text = typeof option.label === "string" ? option.label : String(option.value ?? "");
  return h("span", { title: text }, text);
}

/** 当前图元是否支持实心填充（决定下拉右边那个「实心」开关是否显示）。 */
const shapeToolFillable = computed(() => FILLABLE_SHAPE_KINDS.has(store.shapeKind));

/** 「实心」开关：与「新建图形默认样式」里的同名开关是**同一个** store 字段，
 *  放在这里是为了画之前就能看见状态，不用画完再去右栏找。 */
const shapeToolFilled = computed<boolean>({
  get: () => store.shapeDefault.filled,
  set: (on) => setShapeDefault({ filled: on }),
});

function onShapeKindChange(v: string): void {
  if (v === SHAPE_TOOL_SOLID_TRIANGLE) {
    setShapeKind("triangle");
    setShapeDefault({ filled: true });
    return;
  }
  setShapeKind(v as UiShapeKind);
  // 显式选「三角形」= 要空心三角：关掉实心，否则下拉回显会一直停在「▶ 实心三角」
  if (v === "triangle") setShapeDefault({ filled: false });
}

/** 当前选中的图形（仅当前页才画包围框，避免跨页选中出现幽灵框） */
const selectedShape = computed(() => {
  const i = store.selectedShapeIndex;
  if (i < 0 || i >= store.shapes.length) return null;
  const s = store.shapes[i];
  return s.page === store.pageIndex + 1 ? s : null;
});

/** 选中图形的静止包围框（显示像素）；图形存背景原始像素，×factor 得显示像素 */
const shapeBoxBase = computed(() => {
  const s = selectedShape.value;
  if (!s) return null;
  const f = factor.value;
  return { x: s.x * f, y: s.y * f, w: s.w * f, h: s.h * f };
});

/** 图形调整态（与区域调整态并行独立，互不影响区域路径） */
const shapeAdjusting = ref(false);
const shapeZone = ref<Zone>("outside");
const shapeAdjustIndex = ref(-1);
const shapePress = reactive({ x: 0, y: 0 });
const shapeStartBox = reactive({ x: 0, y: 0, w: 0, h: 0 });
/** 图形调整中的实时几何（显示像素）；静止时用 shapeBoxBase */
const shapeRb = reactive({ x: 0, y: 0, w: 0, h: 0 });
const shapeBox = computed(() => (shapeAdjusting.value ? { ...shapeRb } : shapeBoxBase.value));

/** 新建图形草稿（显示像素；保留拖拽方向，线/箭头靠它传 points + 画预览） */
const shapeDraftLive = ref(false);
const shapeDraft = reactive({ x0: 0, y0: 0, x1: 0, y1: 0 });
/** 草稿虚线框（归一化）：矩形/椭圆/三角/括号/勾/下划线只用它，线/箭头额外叠 SVG */
const shapeDraftBox = computed(() => ({
  x: Math.min(shapeDraft.x0, shapeDraft.x1),
  y: Math.min(shapeDraft.y0, shapeDraft.y1),
  w: Math.abs(shapeDraft.x1 - shapeDraft.x0),
  h: Math.abs(shapeDraft.y1 - shapeDraft.y0),
}));
const shapeDraftIsLine = computed(
  () =>
    store.shapeKind === "line" ||
    store.shapeKind === "arrow" ||
    store.shapeKind === "underline",
);

/** 线/箭头草稿的箭头头（显示像素三角形顶点串），方向跟随当前拖拽方向 */
const shapeDraftArrowHead = computed(() => {
  const { x0, y0, x1, y1 } = shapeDraft;
  const dx = x1 - x0;
  const dy = y1 - y0;
  const len = Math.hypot(dx, dy) || 1;
  const ux = dx / len;
  const uy = dy / len;
  const size = 11;
  const bx = x1 - ux * size;
  const by = y1 - uy * size;
  const px = -uy * size * 0.5;
  const py = ux * size * 0.5;
  return `${x1},${y1} ${bx + px},${by + py} ${bx - px},${by - py}`;
});

/** 已选图形的 8 向命中（与区域调整共用 zoneInBox；用静止包围盒判定） */
function shapeZoneAt(px: number, py: number): Zone {
  const box = shapeBoxBase.value;
  if (!box) return "outside";
  const z = zoneInBox(box, px, py);
  if (z === "outside") return z;
  // 细长图形的退化手柄：短边方向只有 0~16 显示像素时，边/角手柄在几何上无法分辨
  //（水平线 nearT/nearB 重叠会被判成 "t"，窄大括号会被判成 "l"），
  // 统一降级为「整体移动」，避免拖一下就把包围盒拉变形。
  const THIN = 16;
  const zIsCorner = z === "tl" || z === "tr" || z === "bl" || z === "br";
  const zIsVerticalEdge = z === "l" || z === "r";
  const zIsHorizontalEdge = z === "t" || z === "b";
  if (box.w < THIN && (zIsVerticalEdge || zIsCorner)) return "move";
  if (box.h < THIN && (zIsHorizontalEdge || zIsCorner)) return "move";
  return z;
}

/** 显示坐标 → 背景原始像素（图形 API 一律收源坐标） */
function displayToSrc(px: number, py: number): [number, number] {
  const f = factor.value > 0 ? factor.value : 1;
  return [px / f, py / f];
}

/** 图形命中容差（背景原始像素）：固定 8 个**显示**像素再按当前缩放换算，
 *  否则缩小到 25% 时 3 源像素只有 0.75 显示像素，几乎点不中；放大到 400% 又过于灵敏。 */
function shapeHitTolSrc(): number {
  const f = factor.value > 0 ? factor.value : 1;
  return Math.max(3, 8 / f);
}

/** 显示坐标下点到线段的最短距离（线类图形的「拖线身 = 整体平移」判定用）。 */
function distToSegmentDisplay(
  px: number,
  py: number,
  a: [number, number],
  b: [number, number],
): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lenSq = dx * dx + dy * dy;
  if (lenSq <= 1e-9) return Math.hypot(px - a[0], py - a[1]);
  const t = Math.max(0, Math.min(1, ((px - a[0]) * dx + (py - a[1]) * dy) / lenSq));
  return Math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy));
}

/** 选中线类图形（直线 / 箭头 / 下划线）的两个端点（显示坐标）；盒子类返回 null。
 *  线类用端点手柄而不是 8 向手柄：水平/垂直线的包围盒高宽为 0，
 *  8 向判定会退化成「拖边缩放」，既移不动也改不了方向。 */
const shapeLineEnds = computed<[[number, number], [number, number]] | null>(() => {
  const s = selectedShape.value;
  if (!s || !LINE_LIKE_SHAPE_KINDS.has(s.kind)) return null;
  const f = factor.value;
  const pts =
    s.points && s.points.length >= 2
      ? s.points
      : [
          [s.x, s.y],
          [s.x + s.w, s.y + s.h],
        ];
  return [
    [pts[0][0] * f, pts[0][1] * f],
    [pts[1][0] * f, pts[1][1] * f],
  ];
});

/** 端点手柄命中（显示像素，12px 容差）：返回 0 / 1，未命中 -1。 */
function shapeEndpointAt(px: number, py: number): 0 | 1 | -1 {
  const ends = shapeLineEnds.value;
  if (!ends) return -1;
  const d0 = Math.hypot(px - ends[0][0], py - ends[0][1]);
  const d1 = Math.hypot(px - ends[1][0], py - ends[1][1]);
  if (d0 <= 12 && d0 <= d1) return 0;
  if (d1 <= 12) return 1;
  return -1;
}

/** 端点拖动状态：固定另一端，拖动期间只改被拖的一端（松手才写回 store）。 */
const lineDrag = ref<{
  index: number;
  end: 0 | 1;
  origin: [number, number];
  fixed: [number, number];
  current: [number, number];
} | null>(null);

/** 选中线类的实时显示几何：拖动中显示实时端点，静止时就是两端点。 */
const selectedLineOverlay = computed<[[number, number], [number, number]] | null>(() => {
  const ends = shapeLineEnds.value;
  if (!ends) return null;
  const d = lineDrag.value;
  if (!d) return ends;
  return d.end === 0 ? [d.current, d.fixed] : [d.fixed, d.current];
});

/** 进入端点拖动态 */
function beginLineDrag(end: 0 | 1, p: { x: number; y: number }): void {
  const idx = store.selectedShapeIndex;
  const ends = shapeLineEnds.value;
  if (!ends || idx < 0) return;
  const fixed = end === 0 ? ends[1] : ends[0];
  const origin = end === 0 ? ends[0] : ends[1];
  const cp = clampPt(p);
  cancelRegionEdit();
  shapeAdjusting.value = false;
  lineDrag.value = {
    index: idx,
    end,
    origin: [origin[0], origin[1]],
    fixed: [fixed[0], fixed[1]],
    current: [cp.x, cp.y],
  };
}

const cursorStyle = computed(() => {
  if (panning.value) return "grabbing";
  if (lineDrag.value) return "crosshair";
  if (shapeAdjusting.value) return ZONE_CURSORS[shapeZone.value] ?? "default";
  if (adjusting.value) return ZONE_CURSORS[zone.value] ?? "default";
  if (store.shapeMode) {
    // 标注模式下仍可抓已选图形的端点/手柄：给出可拖拽光标提示
    if (hasPreview.value && shapeEndpointAt(hover.x, hover.y) !== -1) return "crosshair";
    if (hasPreview.value && !shapeLineEnds.value && shapeBoxBase.value) {
      const z = shapeZoneAt(hover.x, hover.y);
      if (z !== "outside" && z !== "move") return ZONE_CURSORS[z];
    }
    return "crosshair";
  }
  if (hasPreview.value && shapeEndpointAt(hover.x, hover.y) !== -1) return "crosshair";
  const ends = shapeLineEnds.value;
  if (ends) {
    // 线类：悬停线身显示可拖动光标
    if (distToSegmentDisplay(hover.x, hover.y, ends[0], ends[1]) <= 8) return "move";
  } else if (hasPreview.value && shapeBoxBase.value) {
    const z = shapeZoneAt(hover.x, hover.y);
    if (z !== "outside") return ZONE_CURSORS[z];
  }
  if (hasPreview.value && hasEdit.value) {
    const z = zoneAt(hover.x, hover.y);
    if (z !== "outside") return ZONE_CURSORS[z];
  }
  if (store.regionMode) return "crosshair";
  if (hasPreview.value && factor.value > 0) {
    const [sx, sy] = displayToSrc(hover.x, hover.y);
    if (shapeAtPoint(sx, sy, shapeHitTolSrc()) >= 0) return "pointer";
  }
  if (hasPreview.value && findRegionAt(hover.x, hover.y) >= 0) {
    return "pointer";
  }
  // 放大后空白处可拖拽平移（适应窗口时没有余量，不显示抓手）
  if (hasPreview.value && !hasEdit.value && canPan.value) return "grab";
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

// ---- 缩放与平移 ----
/** 容器（preview-box）内坐标：缩放锚点必须相对容器，而不是会移动的图纸。
 *  注意 preview-box 有 1px 边框：getBoundingClientRect 含边框，draw.x/y 以 padding box 为原点。 */
function boxPoint(e: { clientX: number; clientY: number }): { x: number; y: number } {
  const el = boxEl.value;
  if (!el) return { x: cw.value / 2, y: chh.value / 2 };
  const rect = el.getBoundingClientRect();
  return {
    x: e.clientX - rect.left - el.clientLeft,
    y: e.clientY - rect.top - el.clientTop,
  };
}

function zoomTo(nextScale: number, anchorX: number, anchorY: number): void {
  if (!hasPreview.value) return;
  view.value = zoomAt(
    view.value,
    fit.value,
    natW.value,
    natH.value,
    cw.value,
    chh.value,
    anchorX,
    anchorY,
    nextScale,
  );
}

function zoomIn(): void {
  zoomTo(scale.value * 1.25, cw.value / 2, chh.value / 2);
}

function zoomOut(): void {
  if (atFit.value) return;
  const next = scale.value / 1.25;
  if (next <= fit.value * 1.001) {
    setFit();
    return;
  }
  zoomTo(next, cw.value / 2, chh.value / 2);
}

function setFit(): void {
  resetView();
}

function setActual(): void {
  zoomTo(1, cw.value / 2, chh.value / 2);
}

/** 滚轮 / 触控板缩放：以光标为中心，指数步进；拖框或调整选框期间不改变视口 */
function onWheel(e: WheelEvent): void {
  if (!hasPreview.value) return;
  e.preventDefault();
  if (
    adjusting.value ||
    shapeAdjusting.value ||
    rubberLive.value ||
    shapeDraftLive.value ||
    panning.value
  ) {
    return;
  }
  const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 100 : 1;
  const dy = e.deltaY * unit;
  if (!dy) return;
  const p = boxPoint(e);
  zoomTo(scale.value * Math.exp(-dy * 0.0015), p.x, p.y);
}

function beginPan(e: PointerEvent): void {
  // 调整 / 新建拖拽期间禁止另起平移（例如左键拖动选框时按中键），
  // 否则几何映射会在拖动中途变化，写回坐标出错。
  if (
    !hasPreview.value ||
    !canPan.value ||
    adjusting.value ||
    shapeAdjusting.value ||
    rubberLive.value ||
    shapeDraftLive.value
  ) {
    return;
  }
  panning.value = true;
  panStart.clientX = e.clientX;
  panStart.clientY = e.clientY;
  panStart.moved = false;
  panStart.view = { ...view.value };
  void interactEl.value?.setPointerCapture(e.pointerId);
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

// 记录调整开始时的基准矩形（防止重复读取中间态导致清零漂移）
const startBox = reactive({ x: 0, y: 0, w: 0, h: 0 });

function beginAdjust(z: Zone, p: { x: number; y: number }): void {
  // 必须在 adjusting = true 之前从 ebBase 抓取当前真实矩形
  const base = ebBase.value ?? { x: 0, y: 0, w: 0, h: 0 };
  startBox.x = base.x;
  startBox.y = base.y;
  startBox.w = base.w;
  startBox.h = base.h;

  rb.x = base.x;
  rb.y = base.y;
  rb.w = base.w;
  rb.h = base.h;

  adjusting.value = true;
  zone.value = z;
  const cp = clampPt(p);
  press.x = cp.x;
  press.y = cp.y;
}

function onDown(e: PointerEvent): void {
  if (!hasPreview.value) return;

  // 中键：任何模式下都用于平移（避免与左键的框选/移动/缩放语义冲突）
  if (e.button === 1) {
    e.preventDefault();
    beginPan(e);
    return;
  }
  if (e.button !== 0) return;

  e.preventDefault();
  void interactEl.value?.setPointerCapture(e.pointerId);
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;

  // 0. 图形标注模式：左键拖拽起稿（十字光标，松手才创建；与区域框选互斥）
  if (store.shapeMode) {
    // 例外：**已选中的图形仍可继续微调**——画完一个图形不必先退出标注模式才能拖动/缩放。
    // 线类：拖端点改方向、拖线身整体平移；盒子类：拖角/边缩放、拖内部整体移动。
    // 要画新图形时先点一下选中框外面（会先清选中），再拖。
    if (selectedShape.value) {
      const ep = shapeEndpointAt(p.x, p.y);
      if (ep !== -1) {
        beginLineDrag(ep, p);
        return;
      }
      const ends = shapeLineEnds.value;
      if (ends) {
        if (distToSegmentDisplay(p.x, p.y, ends[0], ends[1]) <= 8) {
          cancelRegionEdit();
          beginShapeAdjust("move", p);
          return;
        }
      } else if (shapeBoxBase.value) {
        const zh = shapeZoneAt(p.x, p.y);
        if (zh !== "outside") {
          cancelRegionEdit();
          beginShapeAdjust(zh, p);
          return;
        }
      }
    }
    // 点中**其它**已有图形：选中它并进入调整，避免标注模式里「只画不选」
    if (hasPreview.value && factor.value > 0) {
      const [sx, sy] = displayToSrc(p.x, p.y);
      const hit = shapeAtPoint(sx, sy, shapeHitTolSrc());
      if (hit >= 0) {
        cancelRegionEdit();
        selectShape(hit);
        const ends2 = shapeLineEnds.value;
        if (ends2) {
          beginShapeAdjust("move", p);
        } else {
          const z2 = shapeZoneAt(p.x, p.y);
          beginShapeAdjust(z2 === "outside" ? "move" : z2, p);
        }
        return;
      }
    }
    clearShapeSelection();
    cancelRegionEdit();
    adjusting.value = false;
    zone.value = "outside";
    rubberLive.value = false;
    shapeAdjusting.value = false;
    shapeZone.value = "outside";
    shapeAdjustIndex.value = -1;
    shapeDraft.x0 = p.x;
    shapeDraft.y0 = p.y;
    shapeDraft.x1 = p.x;
    shapeDraft.y1 = p.y;
    shapeDraftLive.value = true;
    return;
  }

  // 1'. 图形选中优先（非图形模式）：
  //     线类：拖端点改方向 / 拖线身整体平移；盒子类：8 向手柄缩放 / 拖内部整体移动；
  //     再判当前页任意图形命中（选中并进入调整态）；都未命中才回到区域路径。
  //     `regionMode` 是显式框选模式，此时不得抢占：点击一律走下面的区域框选路径。
  if (!store.regionMode && selectedShape.value) {
    const ep = shapeEndpointAt(p.x, p.y);
    if (ep !== -1) {
      beginLineDrag(ep, p);
      return;
    }
    const ends = shapeLineEnds.value;
    if (ends) {
      // 线类：点在线身附近 = 整体平移（不能用 8 向判定，水平线高为 0 时移不动）
      if (distToSegmentDisplay(p.x, p.y, ends[0], ends[1]) <= 8) {
        cancelRegionEdit();
        beginShapeAdjust("move", p);
        return;
      }
    } else if (shapeBoxBase.value) {
      const z = shapeZoneAt(p.x, p.y);
      if (z !== "outside") {
        cancelRegionEdit();
        beginShapeAdjust(z, p);
        return;
      }
    }
  }
  if (!store.regionMode && hasPreview.value && factor.value > 0) {
    const [sx, sy] = displayToSrc(p.x, p.y);
    const shapeHit = shapeAtPoint(sx, sy, shapeHitTolSrc());
    if (shapeHit >= 0) {
      cancelRegionEdit();
      selectShape(shapeHit);
      // 刚选中的图形直接进入可拖动状态：线类拖端点 / 拖线身，盒子类按命中部位
      const ep = shapeEndpointAt(p.x, p.y);
      if (ep !== -1) {
        beginLineDrag(ep, p);
        return;
      }
      const ends = shapeLineEnds.value;
      if (ends) {
        beginShapeAdjust("move", p);
        return;
      }
      const z = shapeZoneAt(p.x, p.y);
      beginShapeAdjust(z === "outside" ? "move" : z, p);
      return;
    }
  }
  // 未命中图形：清图形选中，随后完全走原有区域逻辑（点空白 = 取消选中）
  if (store.selectedShapeIndex >= 0) clearShapeSelection();

  // 1. 已有编辑框优先命中：抓取移动/缩放（即使处于新建模式，对齐 mousePressEvent）
  if (hasEdit.value) {
    const z = zoneAt(p.x, p.y);
    if (z !== "outside") {
      beginAdjust(z, p);
      return;
    }
    // 点击编辑框外：结束当前调整态
    cancelRegionEdit();
  }

  // 2. 新建框选模式
  if (store.regionMode) {
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

  // 3. 普通模式下点击框外：命中当前页区域 → 选中切换；否则空白处可拖拽平移
  const hit = findRegionAt(p.x, p.y);
  if (hit >= 0) {
    jumpToRegion(hit);
    return;
  }
  beginPan(e);
}

/** 新建橡皮筋是否可见（区别于编辑框复用同一几何容器） */
const rubberLive = ref(false);

/** 8 向拖动几何（显示像素）：区域调整与图形调整共用同一套计算，结果钳制在图纸内。
 *  move = 整体平移；其余按命中的边/角移动对应边界，并保证最小显示尺寸 8px。 */
function adjustedRect(
  start: { x: number; y: number; w: number; h: number },
  z: Zone,
  pressPt: { x: number; y: number },
  p: { x: number; y: number },
  areaW: number,
  areaH: number,
): { x: number; y: number; w: number; h: number } {
  const cp = clampPt(p);
  if (z === "move") {
    // 整体平移：基于 start 与鼠标位移计算，钳制在图纸范围内
    const dx = cp.x - pressPt.x;
    const dy = cp.y - pressPt.y;
    return {
      x: Math.max(0, Math.min(start.x + dx, areaW - start.w)),
      y: Math.max(0, Math.min(start.y + dy, areaH - start.h)),
      w: start.w,
      h: start.h,
    };
  }

  let l = start.x;
  let t = start.y;
  let r = start.x + start.w;
  let b = start.y + start.h;

  if (z.includes("l")) l = cp.x;
  if (z.includes("r")) r = cp.x;
  if (z.includes("t")) t = cp.y;
  if (z.includes("b")) b = cp.y;

  const nx = Math.min(l, r);
  const ny = Math.min(t, b);
  const nw = Math.abs(r - l);
  const nh = Math.abs(b - t);

  // 保证最小显示尺寸 8px 并钳制在图纸范围内
  const x = Math.max(0, Math.min(nx, areaW - 8));
  const y = Math.max(0, Math.min(ny, areaH - 8));
  return {
    x,
    y,
    w: Math.max(8, Math.min(nw, areaW - x)),
    h: Math.max(8, Math.min(nh, areaH - y)),
  };
}

function applyAdjust(p: { x: number; y: number }): void {
  Object.assign(
    rb,
    adjustedRect(startBox, zone.value, press, p, draw.value.w, draw.value.h),
  );
}

/** 进入图形调整态：从静止包围框抓基准几何，拖动期间只改 shapeRb，松手才写回 store */
function beginShapeAdjust(z: Zone, p: { x: number; y: number }): void {
  const base = shapeBoxBase.value;
  const index = store.selectedShapeIndex;
  if (!base || index < 0) return;
  shapeAdjustIndex.value = index;
  shapeStartBox.x = base.x;
  shapeStartBox.y = base.y;
  shapeStartBox.w = base.w;
  shapeStartBox.h = base.h;

  shapeRb.x = base.x;
  shapeRb.y = base.y;
  shapeRb.w = base.w;
  shapeRb.h = base.h;

  shapeAdjusting.value = true;
  shapeZone.value = z;
  const cp = clampPt(p);
  shapePress.x = cp.x;
  shapePress.y = cp.y;
}

function applyShapeAdjust(p: { x: number; y: number }): void {
  Object.assign(
    shapeRb,
    adjustedRect(shapeStartBox, shapeZone.value, shapePress, p, draw.value.w, draw.value.h),
  );
}

function onMove(e: PointerEvent): void {
  // 平移用 client 坐标累计：图纸自身在移动，localPoint 会随元素位移变化
  if (panning.value) {
    const dx = e.clientX - panStart.clientX;
    const dy = e.clientY - panStart.clientY;
    if (!panStart.moved && Math.hypot(dx, dy) < 3) return;
    panStart.moved = true;
    view.value = panBy(
      panStart.view,
      dx,
      dy,
      fit.value,
      natW.value,
      natH.value,
      cw.value,
      chh.value,
    );
    return;
  }
  const p = localPoint(e);
  hover.x = p.x;
  hover.y = p.y;
  if (lineDrag.value) {
    const cp = clampPt(p);
    lineDrag.value = { ...lineDrag.value, current: [cp.x, cp.y] };
    return;
  }
  if (shapeAdjusting.value) {
    applyShapeAdjust(p);
    return;
  }
  if (shapeDraftLive.value) {
    const cp = clampPt(p);
    shapeDraft.x1 = cp.x;
    shapeDraft.y1 = cp.y;
    return;
  }
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
  if (panning.value) {
    panning.value = false;
    // 图纸已随平移移动：重新取一次局部坐标，恢复正确的光标形态
    const p = localPoint(e);
    hover.x = p.x;
    hover.y = p.y;
    return;
  }
  const p = localPoint(e);
  // 松手时的真实坐标：快速拖拽时最后一次 pointermove 可能还差几像素，
  // 用它收尾可以让草稿/调整的终点不「短一截」。
  const pEnd = clampPt(p);
  // 线类端点拖动：松手才写回（显示像素位移 <4 视为误触）
  if (lineDrag.value) {
    const d = lineDrag.value;
    // 用松手坐标作为最终端点，再取 overlay（overlay 读的是 lineDrag 的 current）
    lineDrag.value = { ...d, current: [pEnd.x, pEnd.y] };
    const overlay = selectedLineOverlay.value;
    const moved = Math.hypot(pEnd.x - d.origin[0], pEnd.y - d.origin[1]);
    lineDrag.value = null;
    hover.x = p.x;
    hover.y = p.y;
    if (moved >= 4 && overlay) {
      // 防御：拖动期间可能被 Delete / 列表操作删掉或换选中，索引可能已经指向别的图形
      if (d.index === store.selectedShapeIndex && store.shapes[d.index]) {
        const [ax, ay] = displayToSrc(overlay[0][0], overlay[0][1]);
        const [bx, by] = displayToSrc(overlay[1][0], overlay[1][1]);
        setShapeEndpoints(d.index, [ax, ay], [bx, by]);
      }
    }
    return;
  }
  // 图形调整：松手才写回（显示像素增量 <4 视为误触，不写）
  if (shapeAdjusting.value) {
    const index = shapeAdjustIndex.value;
    // 用松手坐标收尾，避免快速拖拽时终点停留在最后一次 pointermove
    applyShapeAdjust(pEnd);
    // 基准用按下时抓取的显示几何（shapeStartBox），不依赖中途重新读取 store
    const moved = Math.max(
      Math.abs(shapeRb.x - shapeStartBox.x),
      Math.abs(shapeRb.y - shapeStartBox.y),
      Math.abs(shapeRb.x + shapeRb.w - (shapeStartBox.x + shapeStartBox.w)),
      Math.abs(shapeRb.y + shapeRb.h - (shapeStartBox.y + shapeStartBox.h)),
    );
    shapeAdjusting.value = false;
    shapeZone.value = "outside";
    shapeAdjustIndex.value = -1;
    hover.x = p.x;
    hover.y = p.y;
    if (moved >= 4 && index >= 0) {
      // 防御：拖动期间若图形被删除 / 选中被切换，索引可能指向别的图形，写回会改错对象
      if (index === store.selectedShapeIndex && store.shapes[index]) {
        const [sx0, sy0] = displayToSrc(shapeRb.x, shapeRb.y);
        const [sx1, sy1] = displayToSrc(shapeRb.x + shapeRb.w, shapeRb.y + shapeRb.h);
        updateShapeGeometry(index, [sx0, sy0, sx1, sy1]);
      }
    }
    return;
  }
  // 图形草稿：位移 <4 显示像素视为误触；线/箭头把起止点一起传下去保留方向
  if (shapeDraftLive.value) {
    shapeDraftLive.value = false;
    // 用松手坐标收尾
    shapeDraft.x1 = pEnd.x;
    shapeDraft.y1 = pEnd.y;
    const dx = shapeDraft.x1 - shapeDraft.x0;
    const dy = shapeDraft.y1 - shapeDraft.y0;
    if (Math.hypot(dx, dy) >= 4) {
      const [sx0, sy0] = displayToSrc(shapeDraft.x0, shapeDraft.y0);
      const [sx1, sy1] = displayToSrc(shapeDraft.x1, shapeDraft.y1);
      const lineLike =
        store.shapeKind === "line" ||
        store.shapeKind === "arrow" ||
        store.shapeKind === "underline";
      addShapeFromRect(
        store.shapeKind,
        [sx0, sy0, sx1, sy1],
        lineLike
          ? [
              [sx0, sy0],
              [sx1, sy1],
            ]
          : undefined,
      );
    }
    return;
  }
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
  panning.value = false;
  // 图形草稿 / 图形调整 / 端点拖动被系统取消：直接丢弃，不写回 store
  shapeAdjusting.value = false;
  shapeZone.value = "outside";
  shapeAdjustIndex.value = -1;
  shapeDraftLive.value = false;
  lineDrag.value = null;
}

/** 双击区域 → 打开属性对话框（修改文字 / 参数）。命中当前页最上层包含点的区域。 */
function onDblClick(e: MouseEvent): void {
  if (!hasPreview.value || store.dialogOpen) return;
  // 标注模式下双击不该弹区域对话框（用户是在画图，不是在编辑文字）
  if (store.shapeMode) return;
  const p = localPoint(e);
  const hit = findRegionAt(p.x, p.y);
  if (hit >= 0) {
    openEditRegionDialog(hit);
  }
}

/** 点击预览视口外侧空白灰色背景：退出区域调整态与选中态 */
function onBoxDown(e: PointerEvent): void {
  if (e.target === boxEl.value) {
    cancelRegionEdit();
    clearShapeSelection();
  }
}

/** Esc：收尾本组件的图形本地状态（草稿 / 进行中的调整，App.vue 的全局 Esc 够不到）。
 *  图形选中与模式切换由 App.vue 的 keydown 统一处理；这里不劫持输入框，也不与它抢语义。 */
function onKeydown(e: KeyboardEvent): void {
  if (e.key !== "Escape") return;
  if (
    store.dialogOpen ||
    store.helpModalOpen ||
    store.aboutModalOpen ||
    store.updateModalOpen ||
    store.onboardingOpen
  ) {
    return;
  }
  if (!shapeDraftLive.value && !shapeAdjusting.value && !lineDrag.value) return;
  const el = e.target as HTMLElement | null;
  if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) {
    return;
  }
  shapeDraftLive.value = false;
  shapeAdjusting.value = false;
  shapeZone.value = "outside";
  shapeAdjustIndex.value = -1;
  lineDrag.value = null;
}
onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  // 窗口失焦时 pointerup 可能收不到：兜底取消进行中的草稿/拖动，避免状态卡死
  window.addEventListener("blur", onCancel);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("blur", onCancel);
});

// 拖拽/调整期间窗口尺寸变化会改变 factor，写回坐标会整体缩放：直接取消本次操作
watch([cw, chh], () => {
  if (shapeDraftLive.value || shapeAdjusting.value || lineDrag.value) onCancel();
});

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
    <ErrorBanner />
    <div
      ref="boxEl"
      class="preview-box"
      :style="{ background: boxBackground }"
      @pointerdown="onBoxDown"
      @wheel="onWheel"
    >
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

          <!-- 图形草稿：虚线框（所有图元）；线/箭头额外用 SVG 画预览直线 + 箭头头，方向可见 -->
          <div
            v-if="shapeDraftLive && (shapeDraftBox.w > 0 || shapeDraftBox.h > 0)"
            class="shape-draft-box"
            :style="pxStyle(shapeDraftBox)"
          />
          <svg v-if="shapeDraftLive && shapeDraftIsLine" class="shape-draft-svg">
            <line
              class="shape-draft-line"
              :x1="shapeDraft.x0"
              :y1="shapeDraft.y0"
              :x2="shapeDraft.x1"
              :y2="shapeDraft.y1"
            />
            <polygon
              v-if="store.shapeKind === 'arrow'"
              class="shape-draft-arrow"
              :points="shapeDraftArrowHead"
            />
          </svg>

          <!-- 选中线类图形（直线 / 箭头 / 下划线）：画一条淡蓝辅助线 + 两个圆形端点手柄。
               线类不用 8 向包围盒：水平线高为 0，8 向判定会退化成「拖边」而移不动。 -->
          <template v-if="selectedLineOverlay">
            <svg class="shape-select-svg">
              <line
                class="shape-select-line"
                :x1="selectedLineOverlay[0][0]"
                :y1="selectedLineOverlay[0][1]"
                :x2="selectedLineOverlay[1][0]"
                :y2="selectedLineOverlay[1][1]"
              />
            </svg>
            <span
              v-for="(pt, i) in selectedLineOverlay"
              :key="i"
              class="shape-endpoint-dot"
              :style="{ left: `${pt[0] - 5}px`, top: `${pt[1] - 5}px` }"
            />
          </template>

          <!-- 选中盒类图形：蓝色虚线包围框 + 8 个白色手柄（纯视觉，命中靠 8px 容差） -->
          <div
            v-else-if="shapeBox"
            class="shape-select-box"
            :style="pxStyle(shapeBox)"
          >
            <span v-for="(h, i) in HANDLE_DOTS" :key="i" class="handle-dot" :style="h" />
          </div>

          <div
            ref="interactEl"
            class="interact-layer"
            :style="{ cursor: cursorStyle }"
            @pointerdown="onDown"
            @pointermove="onMove"
            @pointerup="onUp"
            @pointercancel="onCancel"
            @lostpointercapture="onCancel"
            @dblclick="onDblClick"
          />
        </div>

        <!-- 右下角缩放条：滚轮以光标为中心缩放，中键或空白处拖拽平移 -->
        <div class="zoom-bar" @pointerdown.stop @wheel.stop.prevent>
          <NButton size="tiny" quaternary :disabled="atFit" title="缩小" @click="zoomOut">
            −
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :type="atFit ? 'primary' : 'default'"
            title="适应窗口"
            @click="setFit"
          >
            适应
          </NButton>
          <NButton
            size="tiny"
            quaternary
            :type="atActual ? 'primary' : 'default'"
            title="预览图 1:1（100%）"
            @click="setActual"
          >
            100%
          </NButton>
          <NButton size="tiny" quaternary :disabled="atMax" title="放大" @click="zoomIn">
            +
          </NButton>
          <span class="zoom-percent" title="当前显示比例（CSS 像素 / 预览图像素）">
            {{ zoomPercent }}%
          </span>
        </div>
      </template>
      <QuickStart v-if="!hasContent()" />
      <div v-else-if="!hasPreview" class="empty-hint">
        {{ store.rendering ? "正在生成预览…" : "内容已就绪，点击下方「刷新」重试" }}
      </div>
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
      <NTooltip trigger="hover" placement="top">
        <template #trigger>
          <NButton
            size="small"
            :type="store.shapeMode ? 'primary' : 'default'"
            :ghost="store.shapeMode"
            @click="setShapeMode(!store.shapeMode)"
          >
            ✎ 标注
          </NButton>
        </template>
        进入图形标注模式：在预览图上按住鼠标拖出图形（与「框选文字」互斥）
      </NTooltip>
      <template v-if="store.shapeMode">
        <NTooltip trigger="hover" placement="top">
          <template #trigger>
            <span class="shape-kind-wrap" :title="`当前图元：${shapeToolLabel}`">
              <NSelect
                class="shape-kind-select"
                size="small"
                :value="shapeToolValue"
                :options="SHAPE_KIND_OPTIONS"
                :render-label="renderShapeOptionLabel"
                :menu-props="{ class: 'shape-kind-menu' }"
                @update:value="onShapeKindChange"
              />
            </span>
          </template>
          当前图元：{{ shapeToolLabel }}；在预览图上按住拖出图形，选中后拖手柄调整
        </NTooltip>
        <NTooltip v-if="shapeToolFillable" trigger="hover" placement="top">
          <template #trigger>
            <span class="shape-filled-toggle">
              <NSwitch v-model:value="shapeToolFilled" size="small" />
              <span class="shape-filled-label">实心</span>
            </span>
          </template>
          勾上后画的矩形 / 椭圆 / 三角是实心的（行首 ▶ 就是「实心三角」）
        </NTooltip>
      </template>
      <NButton size="small" :loading="store.rendering" @click="doRender()">↻ 刷新</NButton>
    </div>
  </div>
</template>

<style scoped>
/* 图形标注：翻页行新增按钮/下拉后允许换行，窗口较窄时不挤压页码 */
.pagenav {
  flex-wrap: wrap;
  row-gap: 4px;
}

.shape-kind-select {
  /* 104px 时「▶ 实心三角」会被省略号截断（用户截图反馈）；
     132px 刚好容下 6 个全角字形 + 右侧箭头，又不至于把「刷新」挤出这一行 */
  width: 132px;
  flex: 0 0 auto;
}

/* 触发器外层：给窄屏下的省略号留一个 hover 显示完整文案的入口 */
.shape-kind-wrap {
  display: inline-flex;
  flex: 0 0 auto;
}

/* 标注工具栏的「实心」开关：和下拉同一行，紧凑但不挤压页码 */
.shape-filled-toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
  cursor: pointer;
}

.shape-filled-label {
  font-size: 11.5px;
  color: var(--text-sub);
  white-space: nowrap;
}

/* 新建图形草稿：与 rubber-band 同为蓝色语义，虚线表示尚未落笔；松手即生成 */
.shape-draft-box {
  position: absolute;
  border: 1px dashed var(--rubber-blue);
  background: rgba(74, 144, 255, 0.1);
  pointer-events: none;
}

/* 线/箭头的草稿预览：绝对定位覆盖 draw-area，坐标即显示像素 */
.shape-draft-svg {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
  pointer-events: none;
}

.shape-draft-line {
  stroke: var(--rubber-blue);
  stroke-width: 1.5;
  stroke-dasharray: 6 4;
}

.shape-draft-arrow {
  fill: var(--rubber-blue);
}

/* 选中图形：紫色虚线包围框 + 白色手柄。
   刻意与「文字区域」的蓝色实线编辑框区分，避免两种框同时出现时分不清。 */
.shape-select-box {
  position: absolute;
  border: 2px dashed #7b52d6;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.65);
  background: transparent;
  pointer-events: none;
}

/* 选中线类图形：辅助线 + 两个圆形端点手柄（拖端点改方向，拖线身整体平移） */
.shape-select-svg {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
  pointer-events: none;
}

.shape-select-line {
  stroke: var(--rubber-blue);
  stroke-width: 1.5;
  stroke-dasharray: 6 4;
  opacity: 0.85;
}

.shape-endpoint-dot {
  position: absolute;
  width: 10px;
  height: 10px;
  border: 2px solid var(--rubber-blue);
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.7);
  pointer-events: none;
}
</style>



