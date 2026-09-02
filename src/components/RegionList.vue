<script setup lang="ts">
//! 框选文字区域列表：悬浮列表项 → 预览图临时虚线高亮；
//! 单击 → 跳页并进入调整态；双击 → 打开属性对话框编辑文字与参数。

import { NButton } from "naive-ui";
import {
  clearRegions,
  deleteSelectedRegion,
  getHighlightInfo,
  getRoleBadgeInfo,
  isDarkActive,
  jumpToRegion,
  openEditRegionDialog,
  regionLabel,
  store,
} from "../store";
import type { Region } from "../store";

function getRegionBadge(r: Region) {
  const roleId = r.roleId ?? (r.printed ? 1 : 0);
  const role = store.roles.find((x) => x.id === roleId);
  if (role) {
    return getRoleBadgeInfo(role, isDarkActive());
  }
  const hlInfo = getHighlightInfo(r.highlight);
  if (hlInfo) {
    return {
      label: hlInfo.name,
      icon: hlInfo.icon,
      color: isDarkActive() ? hlInfo.darkColor : hlInfo.color,
      bg: isDarkActive() ? hlInfo.darkBg : hlInfo.bg,
    };
  }
  return {
    label: r.printed ? "打印体" : "手写体",
    icon: r.printed ? "🖨️" : "✍️",
    color: isDarkActive() ? "#94a3b8" : "#64748b",
    bg: isDarkActive() ? "rgba(148, 163, 184, 0.18)" : "rgba(100, 116, 139, 0.12)",
  };
}
</script>

<template>
  <div class="section-title">文字区域（手写 / 打印混排）</div>
  <div class="hint-line">
    勾选「框选文字」后在左侧预览图拖出矩形生成文字；单击列表项在预览中拖动 /
    缩放边框（Esc 或点击空白退出调整）；双击列表项或预览中的区域可重新打开
    对话框修改文字与参数。
  </div>
  <div class="hint-line" style="margin-top: 4px; color: var(--accent)">
    💡 提示：在「笔迹角色管理」中修改角色的字体、墨水颜色或排版扰动，将自动同步更新所有绑定该角色的文字区域。
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
      <span
        class="role-dot"
        :style="{
          backgroundColor: getRegionBadge(r).color,
          boxShadow: `0 0 0 1.5px ${getRegionBadge(r).bg}`,
        }"
        :title="getRegionBadge(r).label"
      />
      <span class="region-label-text">{{ regionLabel(r, i + 1) }}</span>
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

<style scoped>
.role-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 6px;
  flex-shrink: 0;
  display: inline-block;
}

.region-label-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
