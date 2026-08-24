<script setup lang="ts">
//! 框选文字区域列表：悬浮高亮 / 点击跳页进入调整态 / 双击编辑属性。

import { NButton } from "naive-ui";
import {
  clearRegions,
  deleteSelectedRegion,
  jumpToRegion,
  openEditRegionDialog,
  regionLabel,
  setRegionMode,
  store,
} from "../store";
</script>

<template>
  <div class="section-title" style="justify-content: space-between">
    <span>框选文字区域</span>
    <NButton
      size="tiny"
      :type="store.regionMode ? 'primary' : 'default'"
      @click="setRegionMode(!store.regionMode)"
    >
      {{ store.regionMode ? "框选：开" : "框选：关" }}
    </NButton>
  </div>
  <div class="hint-line">
    开启框选后在预览图上拖出矩形；点列表项可在预览中拖动/缩放调整框，双击编辑文字。
  </div>

  <div class="region-list">
    <div
      v-for="(r, i) in store.regions"
      :key="i"
      class="region-item"
      :class="{ 'is-selected': i === store.selectedRegionIndex }"
      :title="r.text"
      @mouseenter="store.highlightIndex = i"
      @mouseleave="store.highlightIndex = -1"
      @click="jumpToRegion(i)"
      @dblclick="openEditRegionDialog(i)"
    >
      {{ regionLabel(r, i) }}
    </div>
    <div v-if="store.regions.length === 0" class="region-empty">（暂无区域）</div>
  </div>

  <div class="field-row" style="margin-top: 6px">
    <NButton size="small" :disabled="store.selectedRegionIndex < 0" @click="deleteSelectedRegion">
      删除选中
    </NButton>
    <NButton size="small" :disabled="store.regions.length === 0" @click="clearRegions">
      清空全部
    </NButton>
  </div>
</template>
