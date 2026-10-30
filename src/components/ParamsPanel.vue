<script setup lang="ts">
//! 右侧参数面板（按任务阶段重排）：
//!   顶部「当前方案」+ 三步（① 内容 / ② 字迹 / ③ 版式）+ 条件出现的「文字区域」。
//!
//! 设计目标：
//! - 预设从第六组提升为顶部常驻摘要，不再是隐藏入口；
//! - 正文编辑、笔迹角色等重控件按需展开（角色默认收起，正文进弹窗）；
//! - 笔画扰动 / 错字 / 边距收进「高级」，首屏只保留主路径；
//! - 文字区域只在文档模式或已有区域时出现，避免空状态噪音。

import { computed, reactive, ref, watch } from "vue";
import {
  NButton,
  NCheckbox,
  NCollapse,
  NCollapseItem,
  NColorPicker,
  NInput,
  NInputNumber,
  NSelect,
  NSlider,
} from "naive-ui";
import FontPicker from "./FontPicker.vue";
import RegionDialog from "./RegionDialog.vue";
import RegionList from "./RegionList.vue";
import RoleManager from "./RoleManager.vue";
import TextEditorModal from "./TextEditorModal.vue";
import QuickChecklist from "./QuickChecklist.vue";
import {
  chooseBackground,
  cleanText,
  currentBackgroundLabel,
  currentFontLabel,
  exportFiles,
  exportPdf,
  fileName,
  importDocx,
  importDocument,
  loadPanelCollapseState,
  loadPresetFromDialog,
  openTextEditor,
  savePanelCollapseState,
  savePresetToDialog,
  selectPreset,
  store,
  workflowMode,
} from "../store";

const presetOptions = computed(() => {
  const options = store.presets.map((p) => ({ label: p.name, value: p.path }));
  // 外部载入的预设不在 presets 列表里：补一个当前项，避免下拉框显示空白
  if (
    store.currentPresetPath &&
    !options.some((o) => o.value === store.currentPresetPath)
  ) {
    options.unshift({
      label: store.currentPresetName || fileName(store.currentPresetPath),
      value: store.currentPresetPath,
    });
  }
  return options;
});

// ---- 阶段折叠状态：localStorage 记忆；首次使用按默认策略 ----
type PanelGroup = "content" | "style" | "layout" | "regions";
const GROUPS: PanelGroup[] = ["content", "style", "layout", "regions"];

const storedCollapse = loadPanelCollapseState();
const collapse = reactive<Record<PanelGroup, boolean>>(
  storedCollapse ?? {
    content: false, // 内容默认展开
    style: true, // 字迹默认折叠（有字体兜底，不强迫先配）
    layout: true, // 版式默认折叠
    regions: true, // 区域默认折叠（仅文档模式出现）
  },
);
watch(collapse, () => savePanelCollapseState({ ...collapse }));

const expandedNames = computed<Array<string | number>>({
  get: () => GROUPS.filter((g) => !collapse[g]),
  set: (vals) => {
    const open = new Set(vals.map(String));
    for (const g of GROUPS) collapse[g] = !open.has(g);
  },
});

const mode = computed(() => workflowMode());
const showRegions = computed(() => store.regions.length > 0 || mode.value === "document");
const autoFont = computed(() => !!store.fontPath && store.fontPath === store.autoFontPath);

const textParaCount = computed(
  () =>
    store.paragraphs.filter(
      (p) => cleanText(p.text).trim() !== "" || (p.runs?.length ?? 0) > 0,
    ).length,
);
const textCharCount = computed(() =>
  store.paragraphs.reduce((n, p) => n + [...cleanText(p.text)].length, 0),
);

const contentSummary = computed(() => {
  if (mode.value === "document") return `文档底图 ${store.docPages?.length ?? 0} 页`;
  if (textParaCount.value > 0) return `${textParaCount.value} 段 · ${textCharCount.value} 字`;
  if (store.regions.length > 0) return `文字区域 ${store.regions.length} 个`;
  return "还没有内容";
});

const styleSummary = computed(() =>
  store.fontPath.trim() ? currentFontLabel() : "未选字体",
);
const layoutSummary = computed(() => `${store.fontSize}px · 行距 ${store.lineSpacing}`);

const rolesOpen = ref(false);
const advancedOpen = ref(false);

// 文档导入后首次出现区域时自动展开，省得用户找不到「文字区域」
watch(
  () => store.regions.length,
  (count, prev) => {
    if (count > 0 && (prev ?? 0) === 0) collapse.regions = false;
  },
);

function openPresetFile(): void {
  void loadPresetFromDialog();
}

function expandRegions(): void {
  collapse.regions = false;
}
</script>

<template>
  <aside class="params-col">
    <div class="params-scroll">
      <!-- ============ 当前方案（常驻） ============ -->
      <div class="setup-card">
        <div class="setup-head">
          <span class="setup-kicker">当前方案</span>
          <span v-if="store.presetDirty" class="preset-dirty">● 已修改</span>
          <span class="setup-spacer" />
          <NButton text size="tiny" @click="openPresetFile()">从文件载入</NButton>
          <span class="setup-dot">·</span>
          <NButton text size="tiny" @click="savePresetToDialog()">存为预设</NButton>
        </div>

        <div class="setup-preset-row">
          <NSelect
            size="small"
            filterable
            placeholder="选择一套现成方案（字体 + 纸张 + 版式）"
            :options="presetOptions"
            :value="store.currentPresetPath || null"
            style="flex: 1; min-width: 0"
            @update:value="(v: string) => selectPreset(v)"
          />
        </div>

        <div class="setup-chips">
          <span class="setup-chip" :class="{ ok: !!store.fontPath.trim() }">
            🖋 {{ currentFontLabel() }}
          </span>
          <span class="setup-chip" :class="{ ok: !!store.backgroundPath.trim() }">
            📄 {{ currentBackgroundLabel() }}
          </span>
        </div>

        <div v-if="autoFont" class="hint-line setup-note">
          已自动选用系统字体，想更像手写可在「② 字迹」里换成自己的字体。
        </div>
        <div v-if="store.presetNotice" class="hint-line setup-note is-warn">
          {{ store.presetNotice }}
        </div>
      </div>

      <QuickChecklist />

      <NCollapse v-model:expanded-names="expandedNames" class="panel-collapse">
        <!-- ============ ① 内容 ============ -->
        <NCollapseItem name="content" title="① 内容">
          <template #header-extra>
            <span class="collapse-count">{{ contentSummary }}</span>
          </template>

          <div class="step-summary">
            <div class="step-summary-main">
              <span v-if="mode === 'empty'" class="step-empty">还没有内容，从下面选一种方式开始</span>
              <span v-else-if="mode === 'document'">
                已导入 {{ store.docPages?.length ?? 0 }} 页文档底图
                <span v-if="store.docStatus" class="step-sub">· {{ store.docStatus }}</span>
              </span>
              <span v-else-if="textParaCount > 0">
                正文 {{ textParaCount }} 段 · {{ textCharCount }} 字
                <span v-if="store.hasTextSelection" class="step-sub">· 已选中文字</span>
              </span>
              <span v-else>
                已创建 {{ store.regions.length }} 个文字区域
              </span>
            </div>
            <NButton
              v-if="mode !== 'document'"
              size="small"
              type="primary"
              secondary
              @click="openTextEditor()"
            >
              {{ mode === 'empty' ? '写文字' : '编辑正文' }}
            </NButton>
          </div>

          <div class="import-entry">
            <div class="import-entry-head">
              <NButton size="small" @click="importDocx()">导入 Word 正文 (.docx)</NButton>
            </div>
            <div class="import-entry-desc">导入为待处理文本，保留文档中的对齐、缩进与高亮 / 角色标记</div>
          </div>

          <div class="import-entry">
            <div class="import-entry-head">
              <NButton size="small" @click="importDocument()">导入 PDF / Word 打印底图</NButton>
              <span class="step-tag">试卷 / 表格</span>
            </div>
            <div class="import-entry-desc">
              逐页作为底图，可选「识别高亮填空」或「纯底图」；识别后可逐区域微调。
            </div>
          </div>

          <div v-if="mode === 'document'" class="step-summary" style="margin-top: 2px">
            <div class="step-summary-main">
              <span class="step-sub">提示：在 Word 里先把「要手写的内容」用高亮标出，另存 PDF 再导入，版式最稳。</span>
            </div>
            <NButton size="small" tertiary @click="expandRegions()">查看区域</NButton>
          </div>
        </NCollapseItem>

        <!-- ============ ② 字迹 ============ -->
        <NCollapseItem name="style" title="② 字迹">
          <template #header-extra>
            <span class="collapse-count">{{ styleSummary }}</span>
          </template>

          <div class="field-row">
            <span class="field-label">手写字体</span>
            <FontPicker
              v-model:value="store.fontPath"
              placeholder="未选择字体（.ttf/.ttc/.otf）"
              style="flex: 1"
            />
          </div>

          <div class="field-row">
            <span class="field-label">墨水颜色</span>
            <NColorPicker
              v-model:value="store.fontColor"
              :show-alpha="false"
              size="small"
              :swatches="['#000000', '#1a1a8c', '#8b0000', '#003366']"
              style="width: 120px"
            />
            <NInput
              v-model:value="store.fontColor"
              size="small"
              placeholder="#RRGGBB"
              style="width: 96px"
            />
          </div>

          <div class="roles-toggle-row">
            <NButton size="tiny" secondary type="primary" @click="rolesOpen = !rolesOpen">
              {{ rolesOpen ? '收起' : '多人笔迹 / 打印混排' }}（{{ store.roles.length }} 个角色）
            </NButton>
            <span class="step-sub">两个人写、红笔批注、保留打印题干时用</span>
          </div>

          <div v-if="rolesOpen" class="role-manager-wrap">
            <RoleManager />
          </div>

          <div class="hint-line" style="margin-bottom: 0">
            角色 0 跟随主字体；打印体默认沿用原文档字体，可在角色里单独指定。
          </div>
        </NCollapseItem>

        <!-- ============ ③ 版式 ============ -->
        <NCollapseItem name="layout" title="③ 版式">
          <template #header-extra>
            <span class="collapse-count">{{ layoutSummary }}</span>
          </template>

          <div v-if="mode !== 'document'" class="field-row">
            <span class="field-label">纸张</span>
            <NInput
              v-model:value="store.backgroundPath"
              size="small"
              placeholder="未选择纸张（png/jpg/webp/bmp）"
            />
            <NButton size="small" @click="chooseBackground()">选择</NButton>
          </div>
          <div v-else class="hint-line" style="margin-top: 0">
            当前使用文档底图；换纸张请重新导入文档，或导入 Word 正文切回文字模式。
          </div>

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

          <div class="advanced-toggle">
            <NButton size="tiny" quaternary @click="advancedOpen = !advancedOpen">
              {{ advancedOpen ? '收起高级参数 ▲' : '高级：笔画扰动 / 写错字 / 边距 ▼' }}
            </NButton>
          </div>

          <template v-if="advancedOpen">
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
          </template>
        </NCollapseItem>

        <!-- ============ ④ 文字区域（条件出现） ============ -->
        <NCollapseItem v-if="showRegions" name="regions" title="文字区域">
          <template #header-extra>
            <span class="collapse-count">{{ store.regions.length }} 个</span>
          </template>
          <RegionList />
        </NCollapseItem>
      </NCollapse>

      <div class="hint-line" style="text-align: left; margin-top: 8px">
        参数改动会自动渲染；需要立即重绘时点预览区下方的「↻ 刷新」。
      </div>
    </div>

    <!-- ============ 主按钮行 ============ -->
    <div class="action-bar">
      <NButton type="primary" @click="exportFiles()">导出图片</NButton>
      <NButton type="primary" secondary @click="exportPdf()">导出 PDF</NButton>
    </div>

    <TextEditorModal />
    <RegionDialog />
  </aside>
</template>
