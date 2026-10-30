<script setup lang="ts">
//! 正文编辑弹窗：把常驻的 190px 编辑器收进弹窗，右栏只留一行可读摘要。
//!
//! 内容与交互完全复用 ParaEditor（对齐 / 首行缩进 / 选区角色标记都在这里）。

import { NButton, NModal } from "naive-ui";
import ParaEditor from "./ParaEditor.vue";
import {
  applyRoleToSelection,
  closeTextEditor,
  focusEmptyArea,
  getRoleBadgeInfo,
  isDarkActive,
  scheduleRender,
  setAlign,
  store,
  toggleIndent,
} from "../store";

function onShowChange(show: boolean): void {
  if (!show) closeTextEditor();
}

function onAfterEnter(): void {
  focusEmptyArea();
}

function done(): void {
  closeTextEditor();
  scheduleRender();
}
</script>

<template>
  <NModal
    :show="store.textEditorOpen"
    preset="card"
    title="编辑正文"
    class="text-editor-modal"
    style="width: 820px; max-width: 94vw"
    :mask-closable="false"
    @update:show="onShowChange"
    @after-enter="onAfterEnter"
  >
    <div class="text-editor-toolbar">
      <NButton size="tiny" @mousedown.prevent @click="setAlign(0)">左对齐</NButton>
      <NButton size="tiny" @mousedown.prevent @click="setAlign(1)">居中</NButton>
      <NButton size="tiny" @mousedown.prevent @click="setAlign(2)">右对齐</NButton>
      <NButton size="tiny" @mousedown.prevent @click="toggleIndent(true)">首行缩进</NButton>
      <NButton size="tiny" @mousedown.prevent @click="toggleIndent(false)">取消缩进</NButton>
      <span class="text-editor-toolbar-hint">回车分段 · 选中文字可交给某个笔迹角色</span>
    </div>

    <div
      v-if="store.hasTextSelection"
      class="field-row role-tag-bar"
      style="flex-wrap: wrap; gap: 4px; margin-bottom: 6px"
    >
      <span class="tag-bar-label">选区角色:</span>
      <button
        v-for="role in store.roles.filter((r) => r.id > 0)"
        :key="role.id"
        type="button"
        class="role-tag-btn"
        :style="{
          backgroundColor: getRoleBadgeInfo(role, isDarkActive()).bg,
          color: getRoleBadgeInfo(role, isDarkActive()).color,
          borderColor: getRoleBadgeInfo(role, isDarkActive()).color,
        }"
        :title="`选区标记为：${role.name}`"
        @mousedown.prevent
        @click="applyRoleToSelection(role.id)"
      >
        {{ getRoleBadgeInfo(role, isDarkActive()).icon }} {{ role.name }}
      </button>
      <NButton size="tiny" tertiary @mousedown.prevent @click="applyRoleToSelection(0)">
        ✕ 清除标记
      </NButton>
    </div>

    <ParaEditor />

    <div class="text-editor-status">{{ store.paraStatus }}</div>

    <template #footer>
      <div class="text-editor-footer">
        <span class="text-editor-tip">
          提示：粘贴长文会按换行自动分段；右侧「文字区域」用于在纸面上单独框选排版。
        </span>
        <NButton type="primary" @click="done()">完成</NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.text-editor-toolbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}

.text-editor-toolbar-hint {
  margin-left: auto;
  font-size: 11.5px;
  color: var(--text-sub);
}

.text-editor-status {
  margin-top: 6px;
  font-size: 11.5px;
  color: var(--text-sub);
}

.text-editor-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.text-editor-tip {
  font-size: 11.5px;
  color: var(--text-sub);
}

.text-editor-modal :deep(.para-editor) {
  height: min(46vh, 420px);
  min-height: 240px;
}
</style>
