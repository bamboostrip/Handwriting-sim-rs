<script setup lang="ts">
//! 区域属性对话框（模态）：布局样式对齐主界面右侧面板。
//!
//! 基础项——区域文字（含对齐/缩进/docx 导入工具行）/ 手写体·打印体 /
//! 起始页 / 打印字体（仅打印体）/ 字号；
//! 折叠面板「排版与扰动覆盖」——字距/扰动/错字率/涂改方式/颜色逐区域自定义，
//! 留空 = 跟随主设置；打印体下扰动类覆盖不生效（引擎强制规整）。
//! 编辑作用于 store.dialogDraft 草稿，确定时统一写回。

import { computed, ref, watch } from "vue";
import {
  NButton,
  NCollapse,
  NCollapseItem,
  NColorPicker,
  NInput,
  NInputNumber,
  NModal,
  NRadioButton,
  NRadioGroup,
  NSelect,
  NTooltip,
  useMessage,
} from "naive-ui";
import { api } from "../api";
import RegionTextEditor from "./RegionTextEditor.vue";
import {
  cancelRegionDialog,
  chooseRegionFont,
  cleanText,
  confirmRegionDialog,
  importDocxToDraft,
  regionHasOverrides,
  store,
} from "../store";
import type { UiParagraph } from "../types";

const message = useMessage();
const advOpen = ref<string[]>([]);
const curRowIndex = ref(0);

// 每次打开对话框时折叠面板收起、重置聚焦行为第一行
watch(
  () => store.dialogOpen,
  (open) => {
    if (open) {
      advOpen.value = [];
      curRowIndex.value = 0;
    }
  },
);

const draft = computed(() => store.dialogDraft);
const isPrinted = computed(() => draft.value?.printed ?? false);
const isNew = computed(() => store.dialogTargetIndex < 0);

const curPara = computed<UiParagraph | undefined>(
  () => draft.value?.paragraphs?.[curRowIndex.value],
);
const currentAlign = computed(() => curPara.value?.align ?? 0);
const currentIndent = computed(() => curPara.value?.indentEm ?? 0);

const curRowStatus = computed(() => {
  const len = draft.value?.paragraphs?.length ?? 0;
  if (len === 0) return "光标定位到行后可用上方按钮设置该行格式";
  const idx = Math.min(curRowIndex.value, len - 1);
  const p = draft.value?.paragraphs?.[idx];
  if (!p) return "光标定位到行后可用上方按钮设置该行格式";
  const alignName = ["左对齐", "居中", "右对齐"][p.align] ?? "左对齐";
  const indentTxt = (p.indentEm ?? 0) > 0 ? "，首行缩进 2 字" : "";
  const text = cleanText(p.text).replace(/\n/g, "");
  const segTxt = text.trim() === "" ? "（空行）" : "";
  return `第 ${idx + 1} 行（${[...text].length} 字）：${alignName}${indentTxt}${segTxt}`;
});

/** 对齐 / 首行缩进：作用于当前聚焦行 */
function setDraftAlign(align: number): void {
  if (curPara.value) {
    curPara.value.align = align as 0 | 1 | 2;
  }
  if (draft.value && draft.value.paragraphs) {
    draft.value.align = draft.value.paragraphs[0]?.align ?? 0;
  }
}

function toggleDraftIndent(): void {
  if (curPara.value) {
    curPara.value.indentEm = (curPara.value.indentEm ?? 0) > 0 ? 0 : 2;
  }
  if (draft.value && draft.value.paragraphs) {
    draft.value.indentEm = draft.value.paragraphs[0]?.indentEm ?? 0;
  }
}

function onUpdateParagraphs(paras: UiParagraph[]): void {
  if (draft.value) {
    draft.value.paragraphs = paras;
    draft.value.text = paras.map((p) => p.text).join("\n");
  }
}

/** 覆盖项是否已有任意设置（折叠标题显示状态徽标） */
const hasAdvValues = computed(() => (draft.value ? regionHasOverrides(draft.value) : false));

/** 涂改方式代理：UI 用 -1 表示跟随主设置，存储用 null */
const strikeoutProxy = computed<number>({
  get: () => draft.value?.miswriteStrikeoutStyleIndex ?? -1,
  set: (v) => {
    if (draft.value) draft.value.miswriteStrikeoutStyleIndex = v >= 0 ? v : null;
  },
});

/** 错字率代理：UI 百分比 0~30，存储 0~1 */
const miswriteRatePctProxy = computed<number | null>({
  get: () => {
    const v = draft.value?.miswriteRate;
    return v != null ? Math.round(v * 1000) / 10 : null;
  },
  set: (v) => {
    if (draft.value) draft.value.miswriteRate = v == null ? null : v / 100;
  },
});

const strikeoutOptions = [
  { label: "跟随主设置", value: -1 },
  { label: "单横线", value: 0 },
  { label: "双横线", value: 1 },
  { label: "斜线", value: 2 },
  { label: "叉号", value: 3 },
];

/** 确定前校验打印字体文件存在（对齐 Python 版字体检查，失败保持对话框打开） */
async function onConfirm(): Promise<void> {
  const fontPath = draft.value?.fontPath.trim() ?? "";
  if (isPrinted.value && fontPath !== "" && !(await api.pathExists(fontPath))) {
    message.warning(`文字区域字体文件不存在：${fontPath}`);
    return;
  }
  confirmRegionDialog();
}
</script>

<template>
  <NModal
    :show="store.dialogOpen"
    preset="card"
    :title="isNew ? '添加文字区域' : '编辑文字区域'"
    style="width: 560px; max-width: 95vw"
    :mask-closable="false"
    :z-index="1000"
    @update:show="(v: boolean) => (v ? undefined : cancelRegionDialog())"
    @esc="cancelRegionDialog"
  >
    <div v-if="draft" class="dlg-body">
      <!-- 工具行：对齐 / 缩进 / 导入 docx（作用于当前光标所在行） -->
      <div class="field-row" style="flex-wrap: wrap; margin-bottom: 2px">
        <NButton size="tiny" :type="currentAlign === 0 ? 'primary' : 'default'" @click="setDraftAlign(0)">左对齐</NButton>
        <NButton size="tiny" :type="currentAlign === 1 ? 'primary' : 'default'" @click="setDraftAlign(1)">居中</NButton>
        <NButton size="tiny" :type="currentAlign === 2 ? 'primary' : 'default'" @click="setDraftAlign(2)">右对齐</NButton>
        <NButton size="tiny" :type="currentIndent > 0 ? 'primary' : 'default'" @click="toggleDraftIndent()">
          {{ currentIndent > 0 ? "取消缩进" : "首行缩进" }}
        </NButton>
        <NButton size="tiny" @click="importDocxToDraft()">导入 docx</NButton>
      </div>

      <div class="hint-line" style="margin: 2px 0 6px">{{ curRowStatus }}</div>

      <RegionTextEditor
        v-if="draft.paragraphs"
        :paragraphs="draft.paragraphs"
        :cur-index="curRowIndex"
        placeholder="输入该区域内要生成的文字，支持多行；回车分段，上方按钮设置当前行对齐/缩进；留空则放弃该区域"
        @update:paragraphs="onUpdateParagraphs"
        @update:cur-index="curRowIndex = $event"
      />

      <!-- 基础参数：统一「标签列 + 控件列」网格 -->
      <div class="dlg-basic-grid">
        <span class="field-label">样式</span>
        <div class="field-control">
          <NRadioGroup v-model:value="draft.printed" size="small">
            <NRadioButton :value="false">手写体</NRadioButton>
            <NRadioButton :value="true">打印体</NRadioButton>
          </NRadioGroup>
        </div>

        <span class="field-label">所在页</span>
        <div class="field-control">
          <NTooltip trigger="hover" placement="top">
            <template #trigger>
              <NInputNumber
                v-model:value="draft.page"
                size="small"
                :min="1"
                :max="999"
                style="width: 100px; flex: none"
                :show-button="false"
              />
            </template>
            该文字区域在第几页渲染（超出框选范围的内容将自然截断）
          </NTooltip>
          <span class="hint-line" style="margin: 0; margin-left: 6px">仅在指定页渲染，超出框选范围的内容自然截断</span>
        </div>

        <span class="field-label">打印字体</span>
        <div class="field-control">
          <NInput
            v-model:value="draft.fontPath"
            size="small"
            :disabled="!isPrinted"
            placeholder="留空使用主字体"
            style="flex: 1"
          />
          <NButton size="small" :disabled="!isPrinted" @click="chooseRegionFont()">选择</NButton>
        </div>
      </div>

      <!-- ======== 折叠：逐区域排版 / 扰动覆盖 ======== -->
      <NCollapse v-model:expanded-names="advOpen" style="margin-top: 10px">
        <NCollapseItem name="adv">
          <template #header>
            排版与扰动覆盖
            <span
              v-if="hasAdvValues"
              style="margin-left: 6px; font-size: 11px; color: var(--accent); background: var(--accent-soft); border-radius: 8px; padding: 0 7px;"
            >已自定义</span>
            <span v-else style="margin-left: 6px; font-size: 11px; color: #9aa8a4">跟随主设置</span>
          </template>

          <div class="hint-line" style="margin: 0 0 10px 0">
            留空即跟随全局设置；打印体下扰动 / 错字类覆盖不生效。
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
              <NInputNumber
                v-model:value="draft.wordSpacing"
                size="small"
                :min="0"
                :max="100"
                :show-button="false"
                placeholder="跟随主设置"
              />
              <span></span>
              <NInputNumber
                v-model:value="draft.wordSpacingSigma"
                size="small"
                :min="0"
                :max="20"
                :show-button="false"
                placeholder="跟随主设置"
              />

              <span class="field-label" style="width: auto">字竖直间距</span>
              <NInputNumber
                v-model:value="draft.lineSpacing"
                size="small"
                :min="0"
                :max="200"
                :show-button="false"
                placeholder="跟随主设置"
              />
              <span></span>
              <NInputNumber
                v-model:value="draft.lineSpacingSigma"
                size="small"
                :min="0"
                :max="20"
                :show-button="false"
                placeholder="跟随主设置"
              />

              <span class="field-label" style="width: auto">字体大小</span>
              <NInputNumber
                v-model:value="draft.fontSize"
                size="small"
                :min="0"
                :max="300"
                :show-button="false"
                placeholder="跟随主设置"
              />
              <span></span>
              <NInputNumber
                v-model:value="draft.fontSizeSigma"
                size="small"
                :min="0"
                :max="20"
                :show-button="false"
                placeholder="跟随主设置"
              />
            </div>
            <div class="hint-line" style="margin: 6px 0 0 0; text-align: right">
              主字号当前为 {{ store.fontSize }}，填 0 或留空跟随主设置。
            </div>
          </div>

          <!-- ============ 笔画扰动 ============ -->
          <div class="group-card">
            <span class="group-legend">笔画扰动</span>
            <div class="sigma-grid" style="grid-template-columns: 84px 1fr">
              <span class="field-label" style="width: auto">水平笔画位移</span>
              <NInputNumber
                v-model:value="draft.perturbXSigma"
                size="small"
                :min="0"
                :max="20"
                :show-button="false"
                placeholder="跟随主设置"
              />
              <span class="field-label" style="width: auto">竖直笔画位移</span>
              <NInputNumber
                v-model:value="draft.perturbYSigma"
                size="small"
                :min="0"
                :max="20"
                :show-button="false"
                placeholder="跟随主设置"
              />
              <span class="field-label" style="width: auto">笔画旋转</span>
              <NInputNumber
                v-model:value="draft.perturbThetaSigma"
                size="small"
                :min="0"
                :max="2"
                :step="0.01"
                :precision="3"
                :show-button="false"
                placeholder="跟随主设置"
              />
            </div>
          </div>

          <!-- ============ 写错字 ============ -->
          <div class="group-card">
            <span class="group-legend">写错字</span>
            <div class="field-row">
              <span class="field-label">错字率</span>
              <NInputNumber
                v-model:value="miswriteRatePctProxy"
                size="small"
                :min="0"
                :max="30"
                :step="0.1"
                :show-button="false"
                placeholder="跟随主设置"
                style="flex: 1"
              >
                <template #suffix>%</template>
              </NInputNumber>
            </div>
            <div class="field-row" style="margin-bottom: 0">
              <span class="field-label">涂改方式</span>
              <NSelect
                v-model:value="strikeoutProxy"
                size="small"
                :options="strikeoutOptions"
                style="flex: 1"
              />
            </div>
          </div>

          <!-- ============ 文字颜色 ============ -->
          <div class="group-card">
            <span class="group-legend">文字颜色</span>
            <div class="field-row" style="margin-bottom: 0">
              <span class="field-label">文字颜色</span>
              <NColorPicker
                v-model:value="draft.fill"
                :show-alpha="false"
                size="small"
                :actions="['clear']"
                :modes="['hex']"
                :swatches="['#000000', '#1a1a8c', '#8b0000', '#003366']"
                style="width: 120px"
              />
              <NInput
                :value="draft.fill ?? ''"
                size="small"
                placeholder="跟随主设置"
                style="width: 110px"
                @update:value="(v: string) => { if (draft) draft.fill = v.trim() ? v.trim() : null; }"
              />
              <NButton
                v-if="draft.fill"
                size="small"
                @click="draft.fill = null"
              >
                重置跟随
              </NButton>
            </div>
          </div>

          <!-- ============ 边距 ============ -->
          <div class="group-card" style="margin-bottom: 0">
            <span class="group-legend">边距</span>
            <div class="margin-grid">
              <span></span>
              <NInputNumber
                v-model:value="draft.marginTop"
                size="small"
                :min="0"
                :max="1000"
                :show-button="false"
                placeholder="0"
              />
              <span></span>

              <NInputNumber
                v-model:value="draft.marginLeft"
                size="small"
                :min="0"
                :max="1000"
                :show-button="false"
                placeholder="0"
              />
              <span class="margin-center-mark">边距</span>
              <NInputNumber
                v-model:value="draft.marginRight"
                size="small"
                :min="0"
                :max="1000"
                :show-button="false"
                placeholder="0"
              />

              <span></span>
              <NInputNumber
                v-model:value="draft.marginBottom"
                size="small"
                :min="0"
                :max="1000"
                :show-button="false"
                placeholder="0"
              />
              <span></span>
            </div>
            <div class="hint-line" style="margin: 6px 0 0 0; text-align: center">
              区域内边距，默认为 0（紧贴框选边界），可按需自定义留白。
            </div>
          </div>
        </NCollapseItem>
      </NCollapse>
    </div>

    <template #footer>
      <div class="action-bar" style="padding-top: 0">
        <NButton :disabled="!draft" @click="cancelRegionDialog()">取消</NButton>
        <NButton type="primary" :disabled="!draft" @click="onConfirm()">确定</NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.dlg-body {
  max-height: calc(85vh - 120px);
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 4px;
}

.dlg-basic-grid {
  display: grid;
  grid-template-columns: 60px 1fr;
  gap: 8px 10px;
  align-items: center;
  margin-top: 10px;
}

.dlg-basic-grid .field-label {
  text-align: right;
  font-size: 12.5px;
  color: var(--text-main);
  white-space: nowrap;
}

.field-control {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

/* 覆盖项中的 group-card 样式（对齐右侧面板） */
.group-card {
  position: relative;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: #fff;
  padding: 14px 12px 10px;
  margin-bottom: 12px;
  margin-top: 6px;
}

.group-card > .group-legend {
  position: absolute;
  top: -9px;
  left: 12px;
  background: #fff;
  padding: 0 6px;
  font-weight: 700;
  font-size: 12px;
  color: var(--accent);
}

.sigma-grid {
  display: grid;
  grid-template-columns: 78px 1fr 12px 1fr;
  gap: 6px 4px;
  align-items: center;
}

.sigma-grid .col-head {
  text-align: center;
  font-weight: 700;
  font-size: 12px;
  color: var(--text-sub);
}
</style>
