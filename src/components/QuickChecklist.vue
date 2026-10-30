<script setup lang="ts">
//! 三步上手清单：不打断、可恢复，状态由真实数据驱动。
//!
//! 字迹（选了字体）→ 内容（有正文或已导入文档）→ 导出（导出过一次）。
//! 全部完成后收成一行「已上手」，可手动关闭。

import { computed } from "vue";
import { NButton } from "naive-ui";
import { dismissChecklist, hasContent, store } from "../store";

const fontDone = computed(() => !!store.fontPath.trim());
const contentDone = computed(() => hasContent());
const exportDone = computed(() => store.hasExported);
const allDone = computed(() => fontDone.value && contentDone.value && exportDone.value);
const visible = computed(() => !store.checklistDismissed);
</script>

<template>
  <div v-if="visible" class="quick-checklist" :class="{ 'is-done': allDone }">
    <template v-if="!allDone">
      <span class="qc-title">上手三步</span>
      <span class="qc-step" :class="{ done: fontDone }">
        <span class="qc-dot">{{ fontDone ? "✓" : "1" }}</span>字迹
      </span>
      <span class="qc-line" />
      <span class="qc-step" :class="{ done: contentDone }">
        <span class="qc-dot">{{ contentDone ? "✓" : "2" }}</span>放入内容
      </span>
      <span class="qc-line" />
      <span class="qc-step" :class="{ done: exportDone }">
        <span class="qc-dot">{{ exportDone ? "✓" : "3" }}</span>导出
      </span>
      <NButton
        text
        size="tiny"
        class="qc-dismiss"
        title="不再显示"
        @click="dismissChecklist()"
      >
        ✕
      </NButton>
    </template>
    <template v-else>
      <span class="qc-step done"><span class="qc-dot">✓</span>已上手：字迹、内容、导出都完成了</span>
      <NButton text size="tiny" class="qc-dismiss" @click="dismissChecklist()">收起</NButton>
    </template>
  </div>
</template>

<style scoped>
.quick-checklist {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  margin-bottom: 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card-bg);
  font-size: 11.5px;
  color: var(--text-sub);
  flex-wrap: wrap;
}

.quick-checklist.is-done {
  border-color: rgba(46, 125, 116, 0.35);
  background: var(--accent-soft);
}

.qc-title {
  font-weight: 700;
  color: var(--text-main);
  margin-right: 2px;
}

.qc-step {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}

.qc-step.done {
  color: var(--accent);
  font-weight: 600;
}

.qc-dot {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  border: 1px solid var(--border);
  font-size: 10px;
  line-height: 1;
  background: var(--panel-bg);
}

.qc-step.done .qc-dot {
  border-color: var(--accent);
  background: var(--accent);
  color: #fff;
}

.qc-line {
  width: 12px;
  height: 1px;
  background: var(--border);
}

.qc-dismiss {
  margin-left: auto;
  opacity: 0.6;
}
</style>
