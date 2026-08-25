<script setup lang="ts">
//! 右侧参数面板：布局沿用原 Slint 版分区（待处理文本 / 框选区域 /
//! 字体·背景·文档底图 / 文字颜色 / 预设 / 排版参数 | σ / 笔画扰动 / 写错字 / 边距），
//! 控件升级为 Naive UI；底部固定「预览 / 导出 / 导出 PDF」主按钮行。

import { computed, ref, watch } from "vue";
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NCheckbox,
  NColorPicker,
  NInput,
  NInputNumber,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NSlider,
  NTooltip,
} from "naive-ui";
import ParaEditor from "./ParaEditor.vue";
import RegionList from "./RegionList.vue";
import { dialogs } from "../api";
import {
  chooseBackground,
  chooseFont,
  doRender,
  exportFiles,
  exportPdf,
  importDocx,
  importDocxToRegion,
  importDocument,
  loadPresetFromDialog,
  regionHasOverrides,
  savePresetToDialog,
  selectPreset,
  setAlign,
  setRegionAlign,
  store,
  toggleIndent,
  toggleRegionIndent,
} from "../store";

const presetOptions = computed(() =>
  store.presets.map((p) => ({ label: p.name, value: p.path })),
);

/** 当前内嵌编辑的区域（editingIndex >= 0 时存在） */
const activeRegion = computed(() =>
  store.editingIndex >= 0 ? (store.regions[store.editingIndex] ?? null) : null,
);
const advOpen = ref<string[]>([]);
watch(
  () => store.editingIndex,
  () => {
    advOpen.value = [];
  },
);

const strikeoutOptions = [
  { label: "跟随主设置", value: -1 },
  { label: "单横线", value: 0 },
  { label: "双横线", value: 1 },
  { label: "斜线", value: 2 },
  { label: "叉号", value: 3 },
];

/** 涂改方式代理：UI 用 -1 表示跟随主设置，存储用 null */
const strikeoutProxy = computed<number>({
  get: () => activeRegion.value?.miswriteStrikeoutStyleIndex ?? -1,
  set: (v) => {
    if (activeRegion.value) {
      activeRegion.value.miswriteStrikeoutStyleIndex = v >= 0 ? v : null;
    }
  },
});

/** 错字率代理：UI 百分比 0~30，存储 0~1 */
const miswriteRatePctProxy = computed<number | null>({
  get: () => {
    const v = activeRegion.value?.miswriteRate;
    return v != null ? Math.round(v * 1000) / 10 : null;
  },
  set: (v) => {
    if (activeRegion.value) {
      activeRegion.value.miswriteRate = v == null ? null : v / 100;
    }
  },
});

async function chooseRegionFontFile(): Promise<void> {
  if (!activeRegion.value || store.editingIndex < 0) return;
  const p = await dialogs.pickFont();
  if (typeof p === "string") activeRegion.value.fontPath = p;
}
</script>

<template>
  <aside class="params-col">
    <div class="params-scroll">
      <!-- ============ 待处理文本 ============ -->
      <div class="section-title">待处理文本</div>
      <div class="field-row" style="flex-wrap: wrap">
        <NButton size="tiny" @click="setAlign(0)">左对齐</NButton>
        <NButton size="tiny" @click="setAlign(1)">居中</NButton>
        <NButton size="tiny" @click="setAlign(2)">右对齐</NButton>
        <NButton size="tiny" @click="toggleIndent(true)">首行缩进</NButton>
        <NButton size="tiny" @click="toggleIndent(false)">取消缩进</NButton>
        <NButton size="tiny" @click="importDocx()">导入 docx</NButton>
      </div>
      <div class="hint-line">{{ store.paraStatus }}</div>
      <ParaEditor />

      <!-- ============ 框选文字区域 ============ -->
      <RegionList />

      <!-- ======== 选中区域的内嵌编辑卡片（替代原模态对话框） ======== -->
      <div v-if="activeRegion" class="group-card" style="margin-top: 12px">
        <span class="group-legend">区域 {{ store.editingIndex + 1 }} · 编辑中</span>
        <div class="field-row" style="flex-wrap: wrap">
          <NButton size="tiny" :type="activeRegion.align === 0 ? 'primary' : 'default'" @click="setRegionAlign(store.editingIndex, 0)">左对齐</NButton>
          <NButton size="tiny" :type="activeRegion.align === 1 ? 'primary' : 'default'" @click="setRegionAlign(store.editingIndex, 1)">居中</NButton>
          <NButton size="tiny" :type="activeRegion.align === 2 ? 'primary' : 'default'" @click="setRegionAlign(store.editingIndex, 2)">右对齐</NButton>
          <NButton size="tiny" :type="activeRegion.indentEm > 0 ? 'primary' : 'default'" @click="toggleRegionIndent(store.editingIndex, activeRegion.indentEm <= 0)">
            {{ activeRegion.indentEm > 0 ? "取消缩进" : "首行缩进" }}
          </NButton>
          <NButton size="tiny" @click="importDocxToRegion(store.editingIndex)">导入 docx</NButton>
        </div>
        <NInput
          v-model:value="activeRegion.text"
          type="textarea"
          size="small"
          placeholder="该区域内要生成的文字，支持多行；留空则不渲染"
          :autosize="{ minRows: 3, maxRows: 6 }"
        />

        <div class="field-row" style="margin-top: 8px">
          <span class="field-label">样式</span>
          <NRadioGroup v-model:value="activeRegion.printed" size="small">
            <NRadioButton :value="false">手写体</NRadioButton>
            <NRadioButton :value="true">打印体</NRadioButton>
          </NRadioGroup>
          <span class="field-label" style="width: 44px">起始页</span>
          <NInputNumber v-model:value="activeRegion.page" size="small" :min="1" :max="999" style="width: 84px" :show-button="false" />
        </div>

        <div class="field-row">
          <span class="field-label">打印字体</span>
          <NInput v-model:value="activeRegion.fontPath" size="small" :disabled="!activeRegion.printed" placeholder="留空使用主字体" />
          <NButton size="small" :disabled="!activeRegion.printed" @click="chooseRegionFontFile()">选择</NButton>
        </div>

        <div class="field-row">
          <span class="field-label">字号</span>
          <NInputNumber v-model:value="activeRegion.fontSize" size="small" :min="0" :max="300" style="width: 92px" placeholder="跟随主设置" />
          <span class="hint-line" style="flex: 1; margin: 0">0 = 跟随主设置（当前 {{ store.fontSize }}）。</span>
        </div>

        <!-- 折叠：逐区域排版 / 扰动覆盖（直接双向绑定区域字段，空值=跟随主设置） -->
        <NCollapse v-model:expanded-names="advOpen" style="margin-top: 4px">
          <NCollapseItem name="adv">
            <template #header>
              排版与扰动覆盖
              <span
                v-if="regionHasOverrides(activeRegion)"
                style="margin-left: 6px; font-size: 11px; color: var(--accent); background: var(--accent-soft); border-radius: 8px; padding: 0 7px;"
              >已自定义</span>
              <span v-else style="margin-left: 6px; font-size: 11px; color: #9aa8a4">跟随主设置</span>
            </template>
            <div class="adv-grid">
              <span class="adv-label">水平间距</span>
              <NInputNumber v-model:value="activeRegion.wordSpacing" size="small" :min="0" :max="100" :show-button="false" placeholder="跟随主设置" />
              <span class="adv-label">间距扰动</span>
              <NInputNumber v-model:value="activeRegion.wordSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

              <span class="adv-label">竖直间距</span>
              <NInputNumber v-model:value="activeRegion.lineSpacing" size="small" :min="0" :max="200" :show-button="false" placeholder="跟随主设置" />
              <span class="adv-label">间距扰动</span>
              <NInputNumber v-model:value="activeRegion.lineSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

              <span class="adv-label">字号扰动</span>
              <NInputNumber v-model:value="activeRegion.fontSizeSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
              <span></span>

              <span class="adv-label">水平扰动</span>
              <NInputNumber v-model:value="activeRegion.perturbXSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
              <span class="adv-label">竖直扰动</span>
              <NInputNumber v-model:value="activeRegion.perturbYSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

              <span class="adv-label">旋转扰动</span>
              <NInputNumber v-model:value="activeRegion.perturbThetaSigma" size="small" :min="0" :max="2" :step="0.01" :precision="3" :show-button="false" placeholder="跟随主设置" />
              <span class="adv-label">错字率 %</span>
              <NInputNumber v-model:value="miswriteRatePctProxy" size="small" :min="0" :max="30" :step="0.1" :show-button="false" placeholder="跟随主设置" />

              <span class="adv-label">涂改方式</span>
              <NSelect v-model:value="strikeoutProxy" size="small" :options="strikeoutOptions" class="span3" />

              <span class="adv-label">文字颜色</span>
              <NColorPicker v-model:value="activeRegion.fill" :show-alpha="false" size="small" :actions="['clear']" :modes="['hex']" class="span3" placeholder="跟随主设置" />
            </div>
          </NCollapseItem>
        </NCollapse>
      </div>

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
  </aside>
</template>

<style scoped>
/* 覆盖项网格：固定标签列 + 弹性输入列，两字段一行保持对齐 */
.adv-grid {
  display: grid;
  grid-template-columns: 68px minmax(0, 1fr) 68px minmax(0, 1fr);
  gap: 10px 8px;
  align-items: center;
}

.adv-label {
  text-align: right;
  font-size: 12px;
  color: var(--text-main);
  white-space: nowrap;
}

.adv-grid :deep(.n-input-number),
.adv-grid :deep(.n-select),
.adv-grid :deep(.n-color-picker) {
  width: 100%;
}

/* 涂改方式 / 文字颜色：输入区横跨右侧三列 */
.span3 {
  grid-column: 2 / -1;
}
</style>
