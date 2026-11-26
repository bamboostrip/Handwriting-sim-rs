<script setup lang="ts">
//! 选中图形的属性编辑：类型 / 线宽 / 抖动 / 颜色（跟随主墨色）/ 填充 / 页码。
//! 所有控件直接写回 store（store 内部负责 scheduleRender），改动即时反映到预览。

import { computed } from "vue";
import { NColorPicker, NSelect, NSlider, NSwitch } from "naive-ui";
import { FILLABLE_SHAPE_KINDS, SHAPE_KIND_LABELS, store, updateShapeStyle } from "../store";
import type { UiShape, UiShapeKind } from "../types";

/** 属性下拉不暴露折线（MVP 不做）；顺序按契约：矩形优先，其次线与括号等。 */
const KIND_ORDER: UiShapeKind[] = [
  "rect",
  "ellipse",
  "triangle",
  "line",
  "arrow",
  "brace_left",
  "brace_right",
  "bracket_left",
  "bracket_right",
  "check",
  "underline",
];
const kindOptions = KIND_ORDER.map((k) => ({ label: SHAPE_KIND_LABELS[k], value: k }));

const index = computed(() => store.selectedShapeIndex);
const shape = computed<UiShape | null>(() => store.shapes[index.value] ?? null);
const fillable = computed(() => !!shape.value && FILLABLE_SHAPE_KINDS.has(shape.value.kind));

/** 统一写入口：未选中 / 越界时什么都不做（属性控件此时也不会渲染）。 */
function patch(p: Partial<UiShape>): void {
  if (index.value < 0 || index.value >= store.shapes.length) return;
  updateShapeStyle(index.value, p);
}

// 控件双向绑定到下面这些可写 computed，避免在模板里散落事件处理逻辑
const kindValue = computed<UiShapeKind>({
  get: () => shape.value?.kind ?? "rect",
  set: (v) => patch({ kind: v }),
});

const thicknessValue = computed<number>({
  get: () => shape.value?.thickness ?? 2.4,
  set: (v) => patch({ thickness: v }),
});

const jitterValue = computed<number>({
  get: () => shape.value?.jitter ?? 1.4,
  set: (v) => patch({ jitter: v }),
});

/** 跟随主墨色 = fill 为 null；关掉时以当前主墨色（store.fontColor）作为初值。 */
const followInk = computed<boolean>({
  get: () => shape.value?.fill == null,
  set: (on) => patch({ fill: on ? null : store.fontColor }),
});

const shapeColor = computed<string | null>({
  get: () => shape.value?.fill ?? store.fontColor,
  set: (v) => patch({ fill: v }),
});

const filledValue = computed<boolean>({
  get: () => shape.value?.filled ?? false,
  set: (v) => patch({ filled: v }),
});

const pageValue = computed<number>({
  get: () => shape.value?.page ?? 1,
  set: (v) => patch({ page: v }),
});

/** 页码选项：1..max(预览页数, 当前页码)，便于把图形移到还没渲染出的新页。 */
const pageOptions = computed(() => {
  const total = Math.max(store.previewPages.length, shape.value?.page ?? 1, 1);
  return Array.from({ length: total }, (_, i) => ({
    label: `第 ${i + 1} 页`,
    value: i + 1,
  }));
});
</script>

<template>
  <div class="group-card shape-props-card">
    <span class="group-legend">图形属性</span>

    <template v-if="shape">
      <div class="field-row">
        <span class="field-label">类型</span>
        <NSelect
          v-model:value="kindValue"
          size="small"
          :options="kindOptions"
          style="flex: 1"
        />
      </div>

      <div class="field-row">
        <span class="field-label">线宽</span>
        <NSlider
          v-model:value="thicknessValue"
          :min="1"
          :max="16"
          :step="0.2"
          style="flex: 1"
        />
        <span class="prop-value">{{ shape.thickness.toFixed(1) }} px</span>
      </div>

      <div class="field-row">
        <span class="field-label">抖动</span>
        <NSlider
          v-model:value="jitterValue"
          :min="0"
          :max="4"
          :step="0.1"
          style="flex: 1"
          title="手绘抖动幅度，0 = 直尺"
        />
        <span class="prop-value">{{ shape.jitter.toFixed(1) }} px</span>
      </div>

      <div class="field-row">
        <span class="field-label">颜色</span>
        <NSwitch v-model:value="followInk" size="small" />
        <span class="prop-hint">跟随主墨色</span>
        <span
          v-if="followInk"
          class="ink-dot"
          :style="{ backgroundColor: store.fontColor }"
          :title="`跟随主墨色 ${store.fontColor}`"
        />
        <NColorPicker
          v-else
          v-model:value="shapeColor"
          :show-alpha="false"
          :modes="['hex']"
          size="small"
          style="width: 104px"
        />
      </div>

      <div v-if="fillable" class="field-row">
        <span class="field-label">填充</span>
        <NSwitch v-model:value="filledValue" size="small" />
        <span class="prop-hint">实心填充（行首 ▶ 就靠它）</span>
      </div>

      <div class="field-row">
        <span class="field-label">页码</span>
        <NSelect
          v-model:value="pageValue"
          size="small"
          :options="pageOptions"
          style="width: 112px"
        />
        <span class="prop-hint">切换会跳到该页</span>
      </div>

      <div class="hint-line shape-props-hint">
        改完自动重绘；叠放顺序在上方列表里用 ↑↓ 调整。
      </div>
    </template>

    <div v-else class="hint-line shape-props-empty">
      在列表或预览图上选中一个图形后，可在这里调类型、线宽、颜色、抖动与填充。
    </div>
  </div>
</template>

<style scoped>
.shape-props-card {
  margin-top: 10px;
  margin-bottom: 0;
}

.prop-value {
  width: 52px;
  flex-shrink: 0;
  text-align: right;
  font-size: 11.5px;
  color: var(--text-sub);
  font-variant-numeric: tabular-nums;
}

.prop-hint {
  font-size: 11px;
  color: var(--text-sub);
  white-space: nowrap;
}

.ink-dot {
  width: 14px;
  height: 14px;
  margin-left: 2px;
  border: 1px solid var(--border);
  border-radius: 50%;
  flex-shrink: 0;
}

.shape-props-hint {
  margin: 2px 0 0;
}

.shape-props-empty {
  margin: 2px 0 0;
  line-height: 1.6;
}
</style>
