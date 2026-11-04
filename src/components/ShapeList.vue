<script setup lang="ts">
//! 图形标注列表：单击列表项切到图形所在页并选中；
//! 每项支持 ↑↓ 调叠放、🗑 删除；顶部提供「清空」与「＋ 给文字区域加框」快路径。
//!
//! 样式沿用现有区域列表（styles.css 的 region-list / region-item），
//! 新组件自己的布局与按钮放进 scoped 样式，避免动全局样式。

import { computed } from "vue";
import { NButton } from "naive-ui";
import {
  addShapeAroundRegion,
  clearShapes,
  deleteShape,
  moveShapeOrder,
  selectShape,
  shapeLabel,
  store,
} from "../store";

/** 可用的文字区域：优先当前选中区域，没选中时退到内嵌编辑中的区域。 */
const regionIndex = computed(() => {
  const selected = store.selectedRegionIndex;
  if (selected >= 0 && selected < store.regions.length) return selected;
  const editing = store.editingIndex;
  if (editing >= 0 && editing < store.regions.length) return editing;
  return -1;
});

const addAroundTitle = computed(() => {
  const idx = regionIndex.value;
  if (idx < 0) return "先在预览图上选中一个文字区域";
  return `用第 ${store.regions[idx].page} 页的文字区域生成一个矩形标注`;
});

/** 切到图形所在页并选中；预览页数还没覆盖该页时夹到最后一页，避免越界。 */
function jumpToShape(index: number): void {
  const s = store.shapes[index];
  if (!s) return;
  const lastPage = Math.max(store.previewPages.length - 1, 0);
  store.pageIndex = Math.min(Math.max(s.page - 1, 0), lastPage);
  selectShape(index);
}

function addAroundRegion(): void {
  const idx = regionIndex.value;
  if (idx < 0) return;
  const before = store.shapes.length;
  addShapeAroundRegion(idx);
  // 区域可能在别的页：生成后跟到新图形所在页，方便立刻确认
  if (store.shapes.length > before) jumpToShape(store.shapes.length - 1);
}
</script>

<template>
  <div class="hint-line shape-hint">
    单击列表项定位；↑↓ 调叠放，🗑 删除。图形属于当前文档，不写进预设。
  </div>

  <div class="shape-toolbar">
    <span class="shape-count">共 {{ store.shapes.length }} 个</span>
    <span class="shape-toolbar-spacer" />
    <NButton size="tiny" :disabled="store.shapes.length === 0" @click="clearShapes()">
      清空
    </NButton>
    <span class="shape-add-wrap" :title="addAroundTitle">
      <NButton
        size="tiny"
        type="primary"
        secondary
        :disabled="regionIndex < 0"
        :title="addAroundTitle"
        @click="addAroundRegion()"
      >
        ＋ 给文字区域加框
      </NButton>
    </span>
  </div>

  <div class="region-list shape-list">
    <div
      v-for="(s, i) in store.shapes"
      :key="i"
      class="region-item shape-item"
      :class="{ 'is-selected': i === store.selectedShapeIndex }"
      :title="shapeLabel(s, i)"
      @click="jumpToShape(i)"
    >
      <span class="shape-label-text">{{ shapeLabel(s, i) }}</span>
      <span class="shape-actions">
        <button
          class="shape-act"
          type="button"
          title="上移一层"
          :disabled="i === 0"
          @click.stop="moveShapeOrder(i, -1)"
        >
          ↑
        </button>
        <button
          class="shape-act"
          type="button"
          title="下移一层"
          :disabled="i === store.shapes.length - 1"
          @click.stop="moveShapeOrder(i, 1)"
        >
          ↓
        </button>
        <button
          class="shape-act shape-act-del"
          type="button"
          title="删除该图形"
          @click.stop="deleteShape(i)"
        >
          🗑
        </button>
      </span>
    </div>
    <div v-if="store.shapes.length === 0" class="region-empty shape-empty">
      在预览图上点「✎ 标注」拖出图形；或选中文字区域后点「＋ 给文字区域加框」
    </div>
  </div>
</template>

<style scoped>
.shape-hint {
  margin-bottom: 6px;
}

.shape-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 6px;
}

.shape-count {
  font-size: 11.5px;
  color: var(--text-sub);
}

.shape-toolbar-spacer {
  flex: 1;
}

.shape-add-wrap {
  display: inline-flex;
}

.shape-list {
  /* 比区域列表稍高一点：每行多了叠放 / 删除按钮，仍保持同类视觉 */
  height: 116px;
}

.shape-item {
  gap: 4px;
  cursor: pointer;
}

.shape-label-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.shape-actions {
  display: inline-flex;
  align-items: center;
  gap: 1px;
  flex-shrink: 0;
}

/* 行内小按钮：默认低调，悬浮提亮；首末项的 ↑ / ↓ 由 disabled 降低存在感 */
.shape-act {
  padding: 1px 3px;
  border: 0;
  border-radius: 4px;
  background: transparent;
  color: var(--text-sub);
  font-size: 11px;
  line-height: 1.2;
  cursor: pointer;
}

.shape-act:hover:not(:disabled) {
  background: var(--hover-bg);
  color: var(--text-main);
}

.shape-act:disabled {
  opacity: 0.3;
  cursor: default;
}

.shape-act-del:hover:not(:disabled) {
  color: #c0392b;
}

.shape-empty {
  line-height: 1.6;
  white-space: normal;
}
</style>
