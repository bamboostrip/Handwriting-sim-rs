<script setup lang="ts">
//! 字体选择器组件：直接编辑路径 + 系统字体快速下拉选择 + 本地字体文件浏览
//!
//! 允许用户：
//! 1. 在输入框中直接查看、选中、编辑、粘贴完整字体文件路径
//! 2. 输入关键字实时模糊联想系统字体
//! 3. 点击「系统字体」按钮通过虚拟滚动 / 搜索弹窗快速选择已安装字体
//! 4. 点击「浏览」按钮调用系统原生文件选择器挑选 .ttf/.ttc/.otf 文件

import { computed, onMounted } from "vue";
import { NAutoComplete, NButton, NPopselect } from "naive-ui";
import { dialogs } from "../api";
import { initSystemFonts, store } from "../store";

const props = withDefaults(
  defineProps<{
    value?: string;
    placeholder?: string;
    disabled?: boolean;
    size?: "small" | "medium" | "tiny";
  }>(),
  {
    value: "",
    placeholder: "未选择字体或输入文件路径",
    disabled: false,
    size: "small",
  }
);

const emit = defineEmits<{
  (e: "update:value", value: string): void;
}>();

const inputSize = computed<"small" | "medium">(() =>
  props.size === "tiny" ? "small" : (props.size as "small" | "medium")
);

onMounted(() => {
  if (store.systemFonts.length === 0) {
    void initSystemFonts();
  }
});

// 系统字体选项供 NPopselect 使用
const systemFontOptions = computed(() =>
  store.systemFonts.map((f) => ({
    label: f.name === f.family ? f.name : `${f.name} (${f.family})`,
    value: f.path,
  }))
);

// 自动补全选项：根据用户在输入框中输入的字符（名称、英文名、文件名或路径）进行过滤
const autoCompleteOptions = computed(() => {
  const q = (props.value || "").trim().toLowerCase();
  if (!q) {
    return store.systemFonts.slice(0, 30).map((f) => ({
      label: f.name === f.family ? `${f.name} — ${f.path}` : `${f.name} (${f.family}) — ${f.path}`,
      value: f.path,
    }));
  }
  const filtered = store.systemFonts.filter(
    (f) =>
      f.name.toLowerCase().includes(q) ||
      f.family.toLowerCase().includes(q) ||
      f.path.toLowerCase().includes(q)
  );
  return filtered.slice(0, 40).map((f) => ({
    label: f.name === f.family ? `${f.name} — ${f.path}` : `${f.name} (${f.family}) — ${f.path}`,
    value: f.path,
  }));
});

// naive-ui 的 NAutoComplete 在点击清除按钮时会以 null 触发 update:value，
// 且从下拉选中选项后会先用选项 value 触发 select、紧接着再用选项 label
// （"名称 — 路径"的展示文本，不是合法字体路径）触发一次 update:value。
// 这里统一拦截：null 归一为空字符串；select 之后的第一次 update:value 丢弃。
let suppressNextInputUpdate = false;

function onInputUpdate(val: string | null) {
  if (suppressNextInputUpdate) {
    suppressNextInputUpdate = false;
    return;
  }
  emit("update:value", val ?? "");
}

function onAutoCompleteSelect(val: string | null) {
  if (val !== undefined && val !== null) {
    suppressNextInputUpdate = true;
    emit("update:value", val);
  }
}

function onSelectSystemFont(val: string | null) {
  if (val !== undefined && val !== null) {
    emit("update:value", val);
  }
}

async function onPickFile() {
  const p = await dialogs.pickFont();
  if (typeof p === "string" && p.trim()) {
    emit("update:value", p.trim());
  }
}
</script>

<template>
  <div class="font-picker">
    <NAutoComplete
      :value="props.value"
      :placeholder="props.placeholder"
      :disabled="props.disabled"
      :size="inputSize"
      :options="autoCompleteOptions"
      clearable
      style="flex: 1; min-width: 0"
      @update:value="onInputUpdate"
      @select="onAutoCompleteSelect"
    />
    <NPopselect
      :value="props.value"
      :options="systemFontOptions"
      :disabled="props.disabled"
      :size="inputSize"
      filterable
      scrollable
      virtual-scroll
      trigger="click"
      @update:value="onSelectSystemFont"
    >
      <NButton
        :size="props.size"
        :disabled="props.disabled"
        secondary
        type="default"
        style="flex-shrink: 0"
      >
        系统字体
      </NButton>
    </NPopselect>
    <NButton
      :size="props.size"
      :disabled="props.disabled"
      style="flex-shrink: 0"
      @click="onPickFile"
    >
      浏览
    </NButton>
  </div>
</template>

<style scoped>
.font-picker {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  min-width: 0;
}
</style>
