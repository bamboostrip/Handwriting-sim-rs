<script setup lang="ts">
//! 区域属性对话框（对应原 RegionDialog）：
//! 基础项——区域文字 / 手写体·打印体 / 起始页 / 打印字体（仅打印体）/ 字号；
//! 折叠面板「排版与扰动覆盖」——字距/扰动/错字率/涂改方式/颜色逐区域自定义，
//! 留空 = 跟随主设置；打印体下扰动类覆盖不生效（引擎强制规整）。

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
import { cancelRegionDialog, chooseRegionFont, confirmRegionDialog, store } from "../store";

const isPrinted = computed(() => store.dialogStyleIndex === 1);
const message = useMessage();
const advOpen = ref<string[]>([]);

// 每次打开对话框时折叠面板收起
watch(
  () => store.dialogOpen,
  (open) => {
    if (open) advOpen.value = [];
  },
);

/** 覆盖项是否已有任意设置（折叠标题显示状态点） */
const hasAdvValues = computed(
  () =>
    store.dialogAdv.wordSpacing != null ||
    store.dialogAdv.lineSpacing != null ||
    store.dialogAdv.wordSpacingSigma != null ||
    store.dialogAdv.lineSpacingSigma != null ||
    store.dialogAdv.fontSizeSigma != null ||
    store.dialogAdv.perturbXSigma != null ||
    store.dialogAdv.perturbYSigma != null ||
    store.dialogAdv.perturbThetaSigma != null ||
    store.dialogAdv.miswriteRatePct != null ||
    store.dialogAdv.strikeoutIndex >= 0 ||
    store.dialogAdv.fill != null,
);

const strikeoutOptions = [
  { label: "跟随主设置", value: -1 },
  { label: "单横线", value: 0 },
  { label: "双横线", value: 1 },
  { label: "斜线", value: 2 },
  { label: "叉号", value: 3 },
];

/** 确定前校验打印字体文件存在（对齐 Python 版字体检查，失败保持对话框打开） */
async function onConfirm(): Promise<void> {
  const fontPath = store.dialogFontPath.trim();
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
    :title="store.dialogTargetIndex >= 0 ? '编辑文字区域' : '添加文字区域'"
    style="width: 540px"
    :mask-closable="false"
    :z-index="1000"
    @update:show="(v: boolean) => (v ? undefined : cancelRegionDialog())"
    @esc="cancelRegionDialog"
  >
    <div class="hint-line" style="margin-top: 0">区域文字</div>
    <NInput
      v-model:value="store.dialogText"
      type="textarea"
      placeholder="输入该区域内要生成的文字，支持多行"
      :autosize="{ minRows: 3, maxRows: 6 }"
    />

    <div class="field-row" style="margin-top: 10px">
      <span class="field-label">样式</span>
      <NRadioGroup v-model:value="store.dialogStyleIndex" size="small">
        <NRadioButton :value="0">手写体</NRadioButton>
        <NRadioButton :value="1">打印体</NRadioButton>
      </NRadioGroup>
      <span class="field-label" style="width: 44px">起始页</span>
      <NTooltip trigger="hover" placement="top">
        <template #trigger>
          <NInputNumber
            v-model:value="store.dialogPage"
            size="small"
            :min="1"
            :max="999"
            style="width: 92px"
            :show-button="false"
          />
        </template>
        区域文字从第几页开始渲染；放不下会延续到后续页
      </NTooltip>
    </div>

    <div class="field-row">
      <span class="field-label">打印字体</span>
      <NInput
        v-model:value="store.dialogFontPath"
        size="small"
        :disabled="!isPrinted"
        placeholder="留空使用主字体"
      />
      <NButton size="small" :disabled="!isPrinted" @click="chooseRegionFont()">选择</NButton>
    </div>

    <div class="field-row">
      <span class="field-label">字号</span>
      <NInputNumber
        v-model:value="store.dialogFontSize"
        size="small"
        :min="0"
        :max="300"
        style="width: 92px"
        placeholder="跟随主设置"
      />
      <span class="hint-line" style="flex: 1; margin: 0">
        主字号当前为 {{ store.fontSize }}，填 0 跟随主设置。
      </span>
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
          <NInputNumber v-model:value="store.dialogAdv.wordSpacing" size="small" :min="0" :max="100" :show-button="false" placeholder="跟随主设置" />
          <span class="adv-label">间距 σ</span>
          <NInputNumber v-model:value="store.dialogAdv.wordSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

          <span class="adv-label">竖直间距</span>
          <NInputNumber v-model:value="store.dialogAdv.lineSpacing" size="small" :min="0" :max="200" :show-button="false" placeholder="跟随主设置" />
          <span class="adv-label">间距 σ</span>
          <NInputNumber v-model:value="store.dialogAdv.lineSpacingSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

          <span class="adv-label">字号 σ</span>
          <NInputNumber v-model:value="store.dialogAdv.fontSizeSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
          <span></span>

          <span class="adv-label">水平位移</span>
          <NInputNumber v-model:value="store.dialogAdv.perturbXSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />
          <span class="adv-label">竖直位移</span>
          <NInputNumber v-model:value="store.dialogAdv.perturbYSigma" size="small" :min="0" :max="20" :show-button="false" placeholder="跟随主设置" />

          <span class="adv-label">笔画旋转</span>
          <NInputNumber
            v-model:value="store.dialogAdv.perturbThetaSigma"
            size="small"
            :min="0"
            :max="2"
            :step="0.01"
            :precision="3"
            placeholder="跟随主设置"
          />
          <span class="adv-label">错字率 %</span>
          <NInputNumber v-model:value="store.dialogAdv.miswriteRatePct" size="small" :min="0" :max="30" :step="0.1" placeholder="跟随主设置" />

          <span class="adv-label">涂改方式</span>
          <NSelect
            v-model:value="store.dialogAdv.strikeoutIndex"
            size="small"
            :options="strikeoutOptions"
            class="span3"
          />

          <span class="adv-label">文字颜色</span>
          <NColorPicker
            v-model:value="store.dialogAdv.fill"
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

    <template #footer>
      <div class="action-bar" style="padding-top: 0">
        <NButton @click="cancelRegionDialog()">取消</NButton>
        <NButton type="primary" @click="onConfirm()">确定</NButton>
      </div>
    </template>
  </NModal>
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
