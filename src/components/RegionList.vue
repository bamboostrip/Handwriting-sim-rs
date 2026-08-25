<script setup lang="ts">
//! 框选文字区域列表：悬浮列表项 → 预览图临时虚线高亮；
//! 单击 → 跳页并进入调整态；双击 → 打开属性对话框编辑文字与参数。

import { NButton } from "naive-ui";
import {
  clearRegions,
  deleteSelectedRegion,
  jumpToRegion,
  openEditRegionDialog,
  regionLabel,
  store,
} from "../store";
</script>

<template>
  <div class="section-title">文字区域（手写 / 打印混排）</div>
  <div class="hint-line">
    勾选「框选文字」后在左侧预览图拖出矩形生成文字；单击列表项在预览中拖动 /
    缩放边框（Esc 或点击空白退出调整）；双击列表项或预览中的区域可重新打开
    对话框修改文字与参数。
  </div>

  <div class="region-list">
    <div
      v-for="(r, i) in store.regions"
      :key="i"
      class="region-item"
      :class="{ 'is-selected': i === store.selectedRegionIndex }"
      :title="`${r.text}（双击编辑）`"
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
    <NButton size="small" :disabled="store.selectedRegionIndex < 0" @click="deleteSelectedRegion()">
      删除选中
    </NButton>
    <NButton size="small" :disabled="store.regions.length === 0" @click="clearRegions()">清空</NButton>
  </div>
</template>
