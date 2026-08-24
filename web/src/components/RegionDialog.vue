<script setup lang="ts">
//! 区域属性对话框（对应原 RegionDialog）：区域文字 / 手写体·打印体 /
//! 起始页 / 打印字体（仅打印体）/ 字号（0 = 跟随主设置）。

import { computed } from "vue";
import { NButton, NInput, NInputNumber, NModal, NRadioButton, NRadioGroup } from "naive-ui";
import { cancelRegionDialog, chooseRegionFont, confirmRegionDialog, store } from "../store";

const isPrinted = computed(() => store.dialogStyleIndex === 1);
</script>

<template>
  <NModal
    :show="store.dialogOpen"
    preset="card"
    :title="store.dialogTargetIndex >= 0 ? '编辑文字区域' : '添加文字区域'"
    style="width: 460px"
    :mask-closable="false"
    :z-index="1000"
    @update:show="(v: boolean) => (v ? undefined : cancelRegionDialog())"
    @esc="cancelRegionDialog"
  >
    <div class="hint-line" style="margin-top: 0">区域文字</div>
    <NInput
      v-model:value="store.dialogText"
      type="textarea"
      placeholder="该矩形内要填写/覆盖的文字，支持多行"
      :autosize="{ minRows: 3, maxRows: 6 }"
    />

    <div class="field-row" style="margin-top: 10px">
      <span class="field-label">样式</span>
      <NRadioGroup v-model:value="store.dialogStyleIndex" size="small">
        <NRadioButton :value="0">手写体</NRadioButton>
        <NRadioButton :value="1">打印体</NRadioButton>
      </NRadioGroup>
      <span class="field-label" style="width: 44px">起始页</span>
      <NInputNumber
        v-model:value="store.dialogPage"
        size="small"
        :min="1"
        :max="999"
        style="width: 92px"
        :show-button="false"
      />
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
      />
      <span class="hint-line" style="flex: 1; margin: 0">
        0 = 跟随主设置；打印体不做笔画扰动、排版规整。
      </span>
    </div>

    <template #footer>
      <div class="action-bar" style="padding-top: 0">
        <NButton @click="cancelRegionDialog()">取消</NButton>
        <NButton type="primary" @click="confirmRegionDialog()">确定</NButton>
      </div>
    </template>
  </NModal>
</template>
