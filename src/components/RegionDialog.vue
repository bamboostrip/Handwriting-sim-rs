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
  confirmRegionDialog,
  importDocxToDraft,
  regionHasOverrides,
  store,
} from "../store";

const message = useMessage();
const advOpen = ref<string[]>([]);

// 每次打开对话框时折叠面板收起
watch(
  () => store.dialogOpen,
  (open) => {
    if (open) advOpen.value = [];
  },
);

const draft = computed(() => store.dialogDraft);
const isPrinted = computed(() => draft.value?.printed ?? false);
const isNew = computed(() => store.dialogTargetIndex < 0);

/** 对齐 / 首行缩进：直接作用于草稿 */
function setDraftAlign(align: number): void {
  if (draft.value) draft.value.align = align;
}
function toggleDraftIndent(): void {
  if (draft.value) draft.value.indentEm = draft.value.indentEm > 0 ? 0 : 2;
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
    style="width: 540px"
    :mask-closable="false"
    :z-index="1000"
    @update:show="(v: boolean) => (v ? undefined : cancelRegionDialog())"
    @esc="cancelRegionDialog"
  >
    <div v-if="draft">
      <!-- 工具行：对齐 / 缩进 / 导入 docx（与主界面待处理文本一致） -->
      <div class="field-row" style="flex-wrap: wrap">
        <NButton size="tiny" :type="draft.align === 0 ? 'primary' : 'default'" @click="setDraftAlign(0)">左对齐</NButton>
        <NButton size="tiny" :type="draft.align === 1 ? 'primary' : 'default'" @click="setDraftAlign(1)">居中</NButton>
        <NButton size="tiny" :type="draft.align === 2 ? 'primary' : 'default'" @click="setDraftAlign(2)">右对齐</NButton>
        <NButton size="tiny" :type="draft.indentEm > 0 ? 'primary' : 'default'" @click="toggleDraftIndent()">
          {{ draft.indentEm > 0 ? "取消缩进" : "首行缩进" }}
        </NButton>
        <NButton size="tiny" @click="importDocxToDraft()">导入 docx</NButton>
      </div>

      <RegionTextEditor
        :text="draft.text"
        :align="draft.align"
        :indent-em="draft.indentEm"
        placeholder="输入该区域内要生成的文字，支持多行；回车分段；留空则放弃该区域"
        @update:text="draft.text = $event"
      />

      <!-- 基础参数：统一「标签列 + 控件列」网格，四行标签右对齐、控件左缘对齐 -->
      <div class="dlg-form">
        <span class="dlg-label">样式</span>
        <NRadioGroup v-model:value="draft.printed" size="small">
          <NRadioButton :value="false">手写体</NRadioButton>
          <NRadioButton :value="true">打印体</NRadioButton>
        </NRadioGroup>

        <span class="dlg-label">起始页</span>
        <div class="dlg-field">
          <NTooltip trigger="hover" placement="top">
            <template #trigger>
              <NInputNumber
                v-model:value="draft.page"
                size="small"
                :min="1"
                :max="999"
                style="width: 92px; flex: none"
                :show-button="false"
              />
            </template>
            区域文字从第几页开始渲染；放不下会延续到后续页
          </NTooltip>
        </div>

        <span class="dlg-label">打印字体</span>
        <div class="dlg-field">
          <NInput
            v-model:value="draft.fontPath"
            size="small"
            :disabled="!isPrinted"
            placeholder="留空使用主字体"
          />
          <NButton size="small" :disabled="!isPrinted" @click="chooseRegionFont()">选择</NButton>
        </div>

        <span class="dlg-label">字号</span>
        <div class="dlg-field">
          <NInputNumber
            v-model:value="draft.fontSize"
            size="small"
            :min="0"
            :max="300"
            style="width: 92px; flex: none"
            placeholder="跟随主设置"
          />
          <span class="hint-line dlg-hint">主字号当前为 {{ store.fontSize }}，填 0 跟随主设置。</span>
        </div>
      </div>

      <!-- ======== 折叠：逐区域排版 / 扰动覆盖 ======== -->
      <NCollapse v-model:expanded-names="advOpen" style="margin-top: 6px">
        <NCollapseItem name="adv">
          <template #header>
            排版与扰动覆盖
            <span
              v-if="hasAdvValues"
              style="margin-left: 6px; font-size: 11px; color: var(--accent); background: var(--accent-soft); border-radius: 8px; padding: 0 7px;"
            >已自定义</span>
            <span v-else style="margin-left: 6px; font-size: 11px; color: #9aa8a4">跟随主设置</span>
          </template>

          <div class="hint-line" style="margin-top: 0">
            留空即跟随左侧全局设置；打印体下扰动 / 错字类覆盖不生效。
          </div>
          <div class="adv-grid">
            <span class="adv-label">水平间距</span>
            <NInputNumber v-model:value="draft.wordSpacing" size="small" :min="0" :max="100" :show-button="false" placeholder="跟随主设置" />
            <span class="adv-label">间距扰动</span>
            <NInputNumber v-model:value="draft.wordSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

            <span class="adv-label">竖直间距</span>
            <NInputNumber v-model:value="draft.lineSpacing" size="small" :min="0" :max="200" :show-button="false" placeholder="跟随主设置" />
            <span class="adv-label">间距扰动</span>
            <NInputNumber v-model:value="draft.lineSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

            <span class="adv-label">字号扰动</span>
            <NInputNumber v-model:value="draft.fontSizeSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
            <span></span>

            <span class="adv-label">水平扰动</span>
            <NInputNumber v-model:value="draft.perturbXSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
            <span class="adv-label">竖直扰动</span>
            <NInputNumber v-model:value="draft.perturbYSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

            <span class="adv-label">旋转扰动</span>
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
            <span class="adv-label">错字率 %</span>
            <NInputNumber v-model:value="miswriteRatePctProxy" size="small" :min="0" :max="30" :step="0.1" :show-button="false" placeholder="跟随主设置" />

            <span class="adv-label">涂改方式</span>
            <NSelect v-model:value="strikeoutProxy" size="small" :options="strikeoutOptions" class="span3" />

            <span class="adv-label">文字颜色</span>
            <NColorPicker
              v-model:value="draft.fill"
              :show-alpha="false"
              size="small"
              :actions="['clear']"
              :modes="['hex']"
              class="span3"
              placeholder="跟随主设置"
            />
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
/* 基础参数网格：固定标签列 + 弹性控件列，四行标签右对齐、控件左缘对齐 */
.dlg-form {
  display: grid;
  grid-template-columns: 64px minmax(0, 1fr);
  gap: 8px 10px;
  align-items: center;
  margin-top: 10px;
}

.dlg-label {
  text-align: right;
  font-size: 12.5px;
  color: var(--text-main);
  white-space: nowrap;
}

.dlg-field {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.dlg-field :deep(.n-input) {
  flex: 1;
  min-width: 0;
}

.dlg-hint {
  flex: 1;
  margin: 0;
}

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
