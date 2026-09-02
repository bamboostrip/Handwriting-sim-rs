<script setup lang="ts">
//! 右侧参数面板：布局沿用原 Slint 版分区（待处理文本 / 框选区域 /
//! 字体·背景·文档底图 / 文字颜色 / 预设 / 排版参数 / 笔画扰动 / 写错字 / 边距），
//! 控件升级为 Naive UI；底部固定「预览 / 导出 / 导出 PDF」主按钮行。

import { computed } from "vue";
import {
  NButton,
  NCheckbox,
  NColorPicker,
  NInput,
  NInputNumber,
  NPopselect,
  NSelect,
  NSlider,
  NTooltip,
} from "naive-ui";
import ParaEditor from "./ParaEditor.vue";
import RegionList from "./RegionList.vue";
import RegionDialog from "./RegionDialog.vue";
import RoleManager from "./RoleManager.vue";
import {
  applyRoleToSelection,
  chooseBackground,
  chooseFont,
  cycleThemePreference,
  doRender,
  exportFiles,
  exportPdf,
  getRoleBadgeInfo,
  importDocx,
  importDocument,
  isDarkActive,
  loadPresetFromDialog,
  openAboutModal,
  savePresetToDialog,
  selectPreset,
  setAlign,
  setThemePreference,
  store,
  toggleIndent,
} from "../store";

const themeOptions = [
  { label: "🖥️ 自动跟随系统", value: "auto" },
  { label: "☀️ 浅色模式", value: "light" },
  { label: "🌙 深色模式", value: "dark" },
];

const presetOptions = computed(() =>
  store.presets.map((p) => ({ label: p.name, value: p.path })),
);
</script>

<template>
  <aside class="params-col">
    <div class="params-scroll">
      <!-- ============ 待处理文本 ============ -->
      <div class="section-title" style="display: flex; justify-content: space-between; align-items: center">
        <span>待处理文本</span>
        <div style="display: flex; align-items: center; gap: 6px">
          <NPopselect
            :value="store.themePreference"
            :options="themeOptions"
            @update:value="(val: any) => setThemePreference(val)"
            trigger="hover"
          >
            <NButton
              size="tiny"
              secondary
              :type="store.themePreference === 'auto' ? 'primary' : isDarkActive() ? 'warning' : 'default'"
              @click="cycleThemePreference()"
              style="padding: 0 7px"
            >
              <template v-if="store.themePreference === 'auto'">
                🖥️ 自动 ({{ isDarkActive() ? '深色' : '浅色' }})
              </template>
              <template v-else-if="store.themePreference === 'dark'">
                🌙 深色
              </template>
              <template v-else>
                ☀️ 浅色
              </template>
            </NButton>
          </NPopselect>
          <NButton size="tiny" secondary type="primary" @click="openAboutModal()">
            ℹ️ 关于 / 检查更新
          </NButton>
        </div>
      </div>



      <div class="field-row" style="flex-wrap: wrap">
        <NButton size="tiny" @click="setAlign(0)">左对齐</NButton>
        <NButton size="tiny" @click="setAlign(1)">居中</NButton>
        <NButton size="tiny" @click="setAlign(2)">右对齐</NButton>
        <NButton size="tiny" @click="toggleIndent(true)">首行缩进</NButton>
        <NButton size="tiny" @click="toggleIndent(false)">取消缩进</NButton>
        <NButton size="tiny" @click="importDocx()">导入 docx</NButton>
      </div>
      <div class="field-row role-tag-bar" style="flex-wrap: wrap; gap: 4px; margin-top: 2px">
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
      <div class="hint-line">{{ store.paraStatus }}</div>
      <ParaEditor />

      <!-- ============ 框选文字区域 ============ -->
      <RegionList />

      <!-- ============ 笔迹角色管理 ============ -->
      <RoleManager />

      <!-- ============ 字体 / 背景 / 文档底图 ============ -->
      <div class="field-row" style="margin-top: 10px">
        <span class="field-label">字体</span>
        <NInput v-model:value="store.fontPath" size="small" placeholder="未选择字体（.ttf/.ttc/.otf）" />
        <NButton size="small" @click="chooseFont()">选择</NButton>
      </div>
      <div class="field-row">
        <span class="field-label">背景</span>
        <NInput
          v-model:value="store.backgroundPath"
          size="small"
          placeholder="未选择背景（png/jpg/webp/bmp）"
        />
        <NButton size="small" @click="chooseBackground()">选择</NButton>
      </div>
      <div class="field-row">
        <span class="field-label">文档底图</span>
        <NTooltip trigger="hover" placement="top">
          <template #trigger>
            <NInput
              :value="store.docStatus"
              size="small"
              readonly
              placeholder="可选：导入 PDF / Word 作为打印底图"
            />
          </template>
          把 PDF / Word 文档的打印预览逐页作为背景（替换当前背景图片），然后在预览上框选需要手写填写的位置
        </NTooltip>
        <NButton size="small" @click="importDocument()">导入</NButton>
      </div>

      <!-- ============ 文字颜色 ============ -->
      <div class="field-row">
        <span class="field-label">文字颜色</span>
        <NColorPicker
          v-model:value="store.fontColor"
          :show-alpha="false"
          size="small"
          :swatches="['#000000', '#1a1a8c', '#8b0000', '#003366']"
          style="width: 120px"
        />
        <NInput v-model:value="store.fontColor" size="small" placeholder="#RRGGBB" style="width: 96px" />
      </div>

      <!-- ============ 预设 ============ -->
      <div class="field-row">
        <span class="field-label">预设</span>
        <NSelect
          size="small"
          filterable
          placeholder="— 选择预设 —"
          :options="presetOptions"
          :value="null"
          @update:value="(v: string) => selectPreset(v)"
        />
        <NButton size="small" @click="loadPresetFromDialog()">载入</NButton>
        <NButton size="small" @click="savePresetToDialog()">保存</NButton>
      </div>

      <!-- ============ 排版参数 ============ -->
      <div class="group-card">
        <span class="group-legend">排版参数</span>
        <div class="sigma-grid">
          <span></span>
          <span class="col-head">数值</span>
          <span></span>
          <span class="col-head">随机扰动</span>

          <span class="field-label" style="width: auto">字水平间距</span>
          <NInputNumber v-model:value="store.wordSpacing" size="small" :min="0" :max="100" :show-button="false" />
          <span></span>
          <NInputNumber v-model:value="store.wordSpacingSigma" size="small" :min="0" :max="20" :show-button="false" />

          <span class="field-label" style="width: auto">字竖直间距</span>
          <NInputNumber v-model:value="store.lineSpacing" size="small" :min="0" :max="200" :show-button="false" />
          <span></span>
          <NInputNumber v-model:value="store.lineSpacingSigma" size="small" :min="0" :max="20" :show-button="false" />

          <span class="field-label" style="width: auto">字体大小</span>
          <NInputNumber v-model:value="store.fontSize" size="small" :min="8" :max="200" :show-button="false" />
          <span></span>
          <NInputNumber v-model:value="store.fontSizeSigma" size="small" :min="0" :max="20" :show-button="false" />
        </div>
      </div>

      <!-- ============ 笔画扰动 ============ -->
      <div class="group-card">
        <span class="group-legend">笔画扰动</span>
        <div class="sigma-grid" style="grid-template-columns: 84px 1fr">
          <span class="field-label" style="width: auto">水平笔画位移</span>
          <NInputNumber v-model:value="store.perturbX" size="small" :min="0" :max="20" :show-button="false" />
          <span class="field-label" style="width: auto">竖直笔画位移</span>
          <NInputNumber v-model:value="store.perturbY" size="small" :min="0" :max="20" :show-button="false" />
          <span class="field-label" style="width: auto">笔画旋转</span>
          <NInput
            v-model:value="store.perturbThetaText"
            size="small"
            placeholder="0.05"
            style="text-align: center"
          />
        </div>
      </div>

      <!-- ============ 写错字 ============ -->
      <div class="group-card">
        <span class="group-legend">写错字</span>
        <div class="field-row">
          <span class="field-label">错字率</span>
          <NSlider v-model:value="store.miswriteRate" :min="0" :max="30" :step="0.1" style="flex: 1" />
          <span style="width: 48px; text-align: center">{{ store.miswriteRate.toFixed(1) }}%</span>
        </div>
        <div class="field-row">
          <span class="field-label">重写方式</span>
          <NSelect
            v-model:value="store.miswriteModeIndex"
            size="small"
            :options="[
              { label: '右上方重写', value: 0 },
              { label: '后文重写', value: 1 },
            ]"
          />
        </div>
        <div class="field-row">
          <span class="field-label">涂改方式</span>
          <NSelect
            v-model:value="store.strikeoutStyleIndex"
            size="small"
            :options="[
              { label: '单横线', value: 0 },
              { label: '双横线', value: 1 },
              { label: '斜线', value: 2 },
              { label: '叉号', value: 3 },
            ]"
          />
        </div>
      </div>

      <!-- ============ 边距 ============ -->
      <div class="group-card">
        <span class="group-legend">边距</span>
        <div class="margin-grid">
          <span></span>
          <NInputNumber v-model:value="store.marginTop" size="small" :min="0" :max="3000" :show-button="false" />
          <span></span>

          <NInputNumber v-model:value="store.marginLeft" size="small" :min="0" :max="3000" :show-button="false" />
          <span class="margin-center-mark">边距</span>
          <NInputNumber v-model:value="store.marginRight" size="small" :min="0" :max="3000" :show-button="false" />

          <span></span>
          <NInputNumber v-model:value="store.marginBottom" size="small" :min="0" :max="3000" :show-button="false" />
          <span></span>
        </div>
        <div class="field-row" style="justify-content: center; margin-bottom: 0; margin-top: 8px">
          <NCheckbox v-model:checked="store.boundsVisible">边界提示(仅预览)</NCheckbox>
          <NColorPicker
            v-model:value="store.boundsColor"
            :show-alpha="false"
            size="small"
            style="width: 72px"
          />
        </div>
      </div>

      <div class="hint-line" style="text-align: left">提示：参数改动会自动防抖渲染，也可手动点「预览」。</div>
    </div>

    <!-- ============ 主按钮行 ============ -->
    <div class="action-bar">
      <NButton type="primary" :loading="store.rendering" @click="doRender()">预览</NButton>
      <NButton type="primary" secondary @click="exportFiles()">导出</NButton>
      <NButton type="primary" secondary @click="exportPdf()">导出 PDF</NButton>
    </div>

    <RegionDialog />
  </aside>
</template>
