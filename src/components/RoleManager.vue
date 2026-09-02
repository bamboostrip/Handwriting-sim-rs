<script setup lang="ts">
//! 笔迹角色管理面板（多角色手写 / 打印混排）
//!
//! 支持为不同角色独立配置：
//! - 角色名称、打印体 / 手写体模式
//! - 专属字体路径（留空使用全局主字体）
//! - 专属墨水颜色（null = 跟随全局设置）
//! - 独立排版与笔画扰动覆盖（字距、行距、字号、位移、旋转、错字率等）
//!
//! 与 Word 高亮标记及 {{角色名:文本}} 语法深度绑定。

import { ref } from "vue";
import {
  NButton,
  NCheckbox,
  NCollapse,
  NCollapseItem,
  NColorPicker,
  NInput,
  NInputNumber,
  NPopconfirm,
  NSelect,
  NTag,
} from "naive-ui";
import {
  addRole,
  chooseRoleFont,
  deleteRole,
  getHighlightInfo,
  getRoleBadgeInfo,
  isDarkActive,
  resetRoles,
  roleHasOverrides,
  store,
} from "../store";
import type { UiHandwritingRole } from "../types";

const expandedRoles = ref<number[]>([0]);

const highlightSelectOptions = [
  { label: "无 / 自定义", value: "" },
  { label: "🟨 黄色 (Yellow)", value: "yellow" },
  { label: "🟩 绿色 (Green)", value: "green" },
  { label: "🟦 青色 (Cyan)", value: "cyan" },
  { label: "🟪 品红 (Magenta)", value: "magenta" },
  { label: "🟦 蓝色 (Blue)", value: "blue" },
  { label: "🟥 红色 (Red)", value: "red" },
  { label: "🟦 深蓝 (DarkBlue)", value: "darkBlue" },
  { label: "🟦 深青 (DarkCyan)", value: "darkCyan" },
  { label: "🟩 深绿 (DarkGreen)", value: "darkGreen" },
  { label: "🟪 深品红 (DarkMagenta)", value: "darkMagenta" },
  { label: "🟥 深红 (DarkRed)", value: "darkRed" },
  { label: "🟨 深黄 (DarkYellow)", value: "darkYellow" },
  { label: "⬛ 浅灰 (LightGray)", value: "lightGray" },
  { label: "⬛ 深灰 (DarkGray)", value: "darkGray" },
  { label: "⬛ 黑色 (Black)", value: "black" },
];

const strikeoutOptions = [
  { label: "跟随主设置", value: -1 },
  { label: "单横线", value: 0 },
  { label: "双横线", value: 1 },
  { label: "斜线", value: 2 },
  { label: "叉号", value: 3 },
];

function toggleRoleExpanded(id: number): void {
  const idx = expandedRoles.value.indexOf(id);
  if (idx >= 0) {
    expandedRoles.value.splice(idx, 1);
  } else {
    expandedRoles.value.push(id);
  }
}

function isRoleExpanded(id: number): boolean {
  return expandedRoles.value.includes(id);
}

function onAddNewRole(): void {
  const newRole = addRole();
  expandedRoles.value.push(newRole.id);
}

function onHighlightChange(role: UiHandwritingRole, val: string): void {
  role.highlight = val ? val : null;
  const hlInfo = getHighlightInfo(role.highlight);
  if (hlInfo && (role.name.startsWith("手写角色") || role.name.startsWith("打印角色") || role.name === `角色 ${role.id}`)) {
    role.name = role.printed ? `打印角色 ${role.id} (${hlInfo.name})` : `手写角色 ${role.id - 1} (${hlInfo.name})`;
  }
}

function onFillInput(role: UiHandwritingRole, val: string): void {
  const trimmed = val.trim();
  role.fill = trimmed ? trimmed : null;
}
</script>

<template>
  <div class="group-card role-manager-card">
    <span class="group-legend">笔迹角色管理 (手写/打印混排)</span>

    <!-- 顶部操作行 -->
    <div class="role-toolbar">
      <div class="role-summary">
        共 <b>{{ store.roles.length }}</b> 个角色
      </div>
      <div class="role-actions">
        <NButton size="tiny" type="primary" secondary @click="onAddNewRole()">
          + 添加角色
        </NButton>
        <NPopconfirm @positive-click="resetRoles()">
          <template #trigger>
            <NButton size="tiny" tertiary>
              重置
            </NButton>
          </template>
          确定将角色列表恢复为默认预设配置吗？
        </NPopconfirm>
      </div>
    </div>

    <!-- 角色列表 -->
    <div class="role-list">
      <div
        v-for="role in store.roles"
        :key="role.id"
        class="role-card"
        :class="{ 'is-expanded': isRoleExpanded(role.id) }"
      >
        <!-- 角色卡片头部 -->
        <div class="role-header" @click="toggleRoleExpanded(role.id)">
          <div class="role-header-left">
            <span
              class="role-badge"
              :style="{
                color: getRoleBadgeInfo(role, isDarkActive()).color,
                background: getRoleBadgeInfo(role, isDarkActive()).bg,
              }"
            >
              {{ getRoleBadgeInfo(role, isDarkActive()).icon }} {{ getRoleBadgeInfo(role, isDarkActive()).label }}
            </span>
            <NInput
              v-model:value="role.name"
              size="tiny"
              placeholder="角色名称"
              style="width: 140px; font-weight: 600"
              @click.stop
            />
          </div>

          <div class="role-header-right" @click.stop>
            <NTag
              size="small"
              :bordered="false"
              :type="role.printed ? 'warning' : 'success'"
              style="cursor: pointer"
              @click="role.printed = !role.printed"
            >
              {{ role.printed ? "🖨️ 打印体" : "✍️ 手写体" }}
            </NTag>

            <span
              v-if="roleHasOverrides(role)"
              class="override-tag"
              title="已自定义专属排版、扰动或高亮颜色"
            >
              ⚙️ 已自定义
            </span>

            <NButton
              size="tiny"
              quaternary
              :disabled="role.id === 0"
              type="error"
              title="删除角色"
              style="padding: 0 4px"
              @click.stop="deleteRole(role.id)"
            >
              ✕
            </NButton>

            <span
              class="expand-arrow"
              :class="{ 'is-open': isRoleExpanded(role.id) }"
              @click.stop="toggleRoleExpanded(role.id)"
            >
              ▼
            </span>
          </div>
        </div>

        <!-- 展开的属性配置区 -->
        <div v-if="isRoleExpanded(role.id)" class="role-body">
          <!-- 打印体切换与基础属性 -->
          <div class="role-row">
            <span class="role-label">角色样式</span>
            <div class="role-control">
              <NCheckbox v-model:checked="role.printed">
                设为打印体（排版整齐，无笔画扰动与错字）
              </NCheckbox>
            </div>
          </div>

          <!-- Word 高亮颜色映射与预览 -->
          <div class="role-row">
            <span class="role-label">Word高亮</span>
            <div class="role-control">
              <NSelect
                :value="role.highlight ?? ''"
                size="small"
                :options="highlightSelectOptions"
                style="width: 160px"
                placeholder="对应Word高亮颜色"
                @update:value="onHighlightChange(role, $event)"
              />
              <span
                v-if="role.highlight && getHighlightInfo(role.highlight)"
                class="highlight-preview-swatch"
                :style="{
                  backgroundColor: isDarkActive() ? getHighlightInfo(role.highlight)!.darkBg : getHighlightInfo(role.highlight)!.bg,
                  color: isDarkActive() ? getHighlightInfo(role.highlight)!.darkColor : getHighlightInfo(role.highlight)!.color,
                  border: '1px solid ' + (isDarkActive() ? getHighlightInfo(role.highlight)!.darkColor : getHighlightInfo(role.highlight)!.color)
                }"
              >
                {{ getHighlightInfo(role.highlight)!.icon }} {{ getHighlightInfo(role.highlight)!.name }}
              </span>
              <NButton
                v-if="role.highlight"
                size="small"
                quaternary
                @click="role.highlight = null"
              >
                清除
              </NButton>
            </div>
          </div>

          <!-- 字体路径 -->
          <div class="role-row">
            <span class="role-label">专属字体</span>
            <div class="role-control">
              <NInput
                v-model:value="role.fontPath"
                size="small"
                placeholder="留空使用全局主字体"
                style="flex: 1"
              />
              <NButton size="small" @click="chooseRoleFont(role.id)">选择</NButton>
              <NButton
                v-if="role.fontPath"
                size="small"
                quaternary
                @click="role.fontPath = ''"
              >
                清除
              </NButton>
            </div>
          </div>

          <!-- 墨水颜色 -->
          <div class="role-row">
            <span class="role-label">墨水颜色</span>
            <div class="role-control">
              <NColorPicker
                v-model:value="role.fill"
                :show-alpha="false"
                size="small"
                :actions="['clear']"
                :modes="['hex']"
                :swatches="['#000000', '#1a1a8c', '#8b0000', '#003366']"
                style="width: 120px"
              />
              <NInput
                :value="role.fill ?? ''"
                size="small"
                placeholder="跟随全局"
                style="width: 100px"
                @update:value="onFillInput(role, $event)"
              />
              <NButton
                v-if="role.fill"
                size="small"
                quaternary
                @click="role.fill = null"
              >
                重置跟随
              </NButton>
            </div>
          </div>

          <!-- 折叠：专属排版与扰动覆盖 -->
          <NCollapse style="margin-top: 6px">
            <NCollapseItem title="高级排版与扰动覆盖 (可选)" name="overrides">
              <div v-if="role.printed" class="hint-line" style="color: var(--text-sub); margin-bottom: 8px">
                ℹ️ 当前为打印体模式，排版整齐规整；扰动与错字设置将被引擎自动跳过。
              </div>

              <!-- 排版参数 -->
              <div class="role-subcard">
                <span class="role-subcard-title">排版参数覆盖</span>
                <div class="sigma-grid">
                  <span></span>
                  <span class="col-head">数值</span>
                  <span></span>
                  <span class="col-head">随机扰动</span>

                  <span class="field-label" style="width: auto">字水平间距</span>
                  <NInputNumber
                    v-model:value="role.wordSpacing"
                    size="small"
                    :min="0"
                    :max="100"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                  <span></span>
                  <NInputNumber
                    v-model:value="role.wordSpacingSigma"
                    size="small"
                    :min="0"
                    :max="20"
                    :show-button="false"
                    placeholder="跟随全局"
                  />

                  <span class="field-label" style="width: auto">字竖直间距</span>
                  <NInputNumber
                    v-model:value="role.lineSpacing"
                    size="small"
                    :min="0"
                    :max="200"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                  <span></span>
                  <NInputNumber
                    v-model:value="role.lineSpacingSigma"
                    size="small"
                    :min="0"
                    :max="20"
                    :show-button="false"
                    placeholder="跟随全局"
                  />

                  <span class="field-label" style="width: auto">字体大小</span>
                  <NInputNumber
                    v-model:value="role.fontSize"
                    size="small"
                    :min="8"
                    :max="300"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                  <span></span>
                  <NInputNumber
                    v-model:value="role.fontSizeSigma"
                    size="small"
                    :min="0"
                    :max="20"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                </div>
              </div>

              <!-- 笔画扰动 -->
              <div v-if="!role.printed" class="role-subcard">
                <span class="role-subcard-title">笔画扰动覆盖</span>
                <div class="sigma-grid" style="grid-template-columns: 84px 1fr">
                  <span class="field-label" style="width: auto">水平笔画位移</span>
                  <NInputNumber
                    v-model:value="role.perturbXSigma"
                    size="small"
                    :min="0"
                    :max="20"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                  <span class="field-label" style="width: auto">竖直笔画位移</span>
                  <NInputNumber
                    v-model:value="role.perturbYSigma"
                    size="small"
                    :min="0"
                    :max="20"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                  <span class="field-label" style="width: auto">笔画旋转</span>
                  <NInputNumber
                    v-model:value="role.perturbThetaSigma"
                    size="small"
                    :min="0"
                    :max="2"
                    :step="0.01"
                    :precision="3"
                    :show-button="false"
                    placeholder="跟随全局"
                  />
                </div>
              </div>

              <!-- 写错字 -->
              <div v-if="!role.printed" class="role-subcard" style="margin-bottom: 0">
                <span class="role-subcard-title">写错字覆盖</span>
                <div class="field-row">
                  <span class="field-label">错字率</span>
                  <NInputNumber
                    :value="role.miswriteRate != null ? Math.round(role.miswriteRate * 1000) / 10 : null"
                    size="small"
                    :min="0"
                    :max="30"
                    :step="0.1"
                    :show-button="false"
                    placeholder="跟随全局"
                    style="flex: 1"
                    @update:value="(v) => role.miswriteRate = v == null ? null : v / 100"
                  >
                    <template #suffix>%</template>
                  </NInputNumber>
                </div>
                <div class="field-row" style="margin-bottom: 0">
                  <span class="field-label">涂改方式</span>
                  <NSelect
                    :value="role.miswriteStrikeoutStyleIndex ?? -1"
                    size="small"
                    :options="strikeoutOptions"
                    style="flex: 1"
                    @update:value="(v: number) => role.miswriteStrikeoutStyleIndex = v >= 0 ? v : null"
                  />
                </div>
              </div>
            </NCollapseItem>
          </NCollapse>
        </div>
      </div>
    </div>

    <!-- 混排语法提示 -->
    <div class="hint-line role-footer-hint">
      💡 <b>混排提示</b>：在 Word 中使用任意高亮颜色（黄/绿/青/品红/蓝/红等15种标准高亮）或在文本中书写 <code>&#123;&#123;角色名:内容&#125;&#125;</code> / <code>&#123;&#123;打印:内容&#125;&#125;</code> 即可实现多笔迹混排。
    </div>
  </div>
</template>

<style scoped>
.role-manager-card {
  margin-top: 12px;
  margin-bottom: 12px;
}

.role-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.role-summary {
  font-size: 12px;
  color: var(--text-sub);
}

.role-actions {
  display: flex;
  gap: 6px;
}

.role-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.role-card {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--input-bg);
  overflow: hidden;
  transition: all 0.2s;
}

.role-card:hover {
  border-color: var(--accent);
}

.role-card.is-expanded {
  border-color: var(--accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.05);
}

.role-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 6px 8px;
  cursor: pointer;
  user-select: none;
  background: var(--card-bg);
}

.role-header-left {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.role-badge {
  font-size: 11px;
  font-weight: 600;
  padding: 1px 6px;
  border-radius: 4px;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.role-header-right {
  display: flex;
  align-items: center;
  gap: 6px;
}

.override-tag {
  font-size: 10.5px;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 1px 5px;
  border-radius: 4px;
}

.expand-arrow {
  font-size: 10px;
  color: var(--text-sub);
  transition: transform 0.2s;
  display: inline-block;
  padding: 0 2px;
}

.expand-arrow.is-open {
  transform: rotate(180deg);
}

.role-body {
  padding: 10px;
  border-top: 1px solid var(--row-border);
  background: var(--panel-bg);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.role-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.role-label {
  width: 58px;
  flex-shrink: 0;
  text-align: right;
  font-size: 12px;
  color: var(--text-main);
}

.role-control {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.highlight-preview-swatch {
  font-size: 11px;
  font-weight: 600;
  padding: 2px 7px;
  border-radius: 4px;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 3px;
}

.role-subcard {
  position: relative;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--card-bg);
  padding: 10px 8px 8px;
  margin-bottom: 8px;
  margin-top: 6px;
}

.role-subcard-title {
  position: absolute;
  top: -8px;
  left: 8px;
  background: var(--panel-bg);
  padding: 0 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--accent);
}

.role-footer-hint {
  margin-top: 8px;
  margin-bottom: 2px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-sub);
}

.role-footer-hint code {
  background: var(--hover-bg);
  padding: 1px 4px;
  border-radius: 3px;
  font-family: monospace;
  font-size: 10.5px;
  color: var(--accent);
}
</style>
