<script setup lang="ts">
//! 空状态开始卡：不输入任何内容时，把「下一步」直接摆在预览区中央。
//!
//! 四个入口分别对应：写文字 / 导入 Word 正文 / 导入文档底图 / 看示例。
//! 字体缺失时在卡片内给出兜底按钮，而不是让用户去状态栏找报错。

import { computed, ref } from "vue";
import { NButton } from "naive-ui";
import {
  chooseBackground,
  chooseFont,
  doRender,
  ensureUsableFont,
  importDocx,
  importDocument,
  loadSample,
  openTextEditor,
  store,
} from "../store";

const busy = ref<"" | "docx" | "document" | "sample" | "font" | "paper">("");
const hasFont = computed(() => !!store.fontPath.trim());
const hasPaper = computed(() => !!store.backgroundPath.trim());
const autoFont = computed(
  () => !!store.fontPath && store.fontPath === store.autoFontPath,
);

async function run(kind: "docx" | "document" | "sample", action: () => Promise<void>) {
  if (busy.value) return;
  busy.value = kind;
  try {
    await action();
  } finally {
    busy.value = "";
  }
}

async function useSystemFont(): Promise<void> {
  if (busy.value) return;
  busy.value = "font";
  try {
    await ensureUsableFont();
    await doRender();
  } finally {
    busy.value = "";
  }
}

async function pickFont(): Promise<void> {
  if (busy.value) return;
  busy.value = "font";
  try {
    await chooseFont();
    if (store.fontPath.trim()) await doRender();
  } finally {
    busy.value = "";
  }
}

async function pickPaper(): Promise<void> {
  if (busy.value) return;
  busy.value = "paper";
  try {
    await chooseBackground();
    if (store.backgroundPath.trim()) await doRender();
  } finally {
    busy.value = "";
  }
}
</script>

<template>
  <div class="quick-start">
    <div class="quick-start-card">
      <div class="quick-start-kicker">手写模拟器</div>
    <h2 class="quick-start-title">把文字变成手写，三步就好</h2>
    <p class="quick-start-sub">选字迹与纸张 → 放入内容 → 导出图片或 PDF</p>

    <div class="quick-start-actions">
      <NButton
        type="primary"
        size="large"
        class="quick-start-main"
        @click="openTextEditor()"
      >
        ✍️ 写一段文字
      </NButton>
      <NButton
        size="large"
        :loading="busy === 'docx'"
        @click="run('docx', importDocx)"
      >
        导入 Word 正文
      </NButton>
      <NButton
        size="large"
        :loading="busy === 'document'"
        @click="run('document', importDocument)"
      >
        导入试卷 / 表格底图
      </NButton>
    </div>

    <div class="quick-start-secondary">
      <NButton
        text
        type="primary"
        :loading="busy === 'sample'"
        @click="run('sample', loadSample)"
      >
        先看 30 秒示例 →
      </NButton>
      <span class="quick-start-hint">示例会直接生成一张成品，不动你的文件。</span>
    </div>

    <div v-if="!hasFont || !hasPaper" class="quick-start-warn">
      <span v-if="!hasFont">还差一款手写字体：软件不内置商业字体。</span>
      <span v-else>还差一张纸张：选一张信纸或纯白底图就能开始。</span>
      <div class="quick-start-warn-actions">
        <template v-if="!hasFont">
          <NButton size="tiny" type="primary" :loading="busy === 'font'" @click="useSystemFont()">
            使用系统楷体
          </NButton>
          <NButton size="tiny" :disabled="busy === 'font'" @click="pickFont()">
            选择字体文件
          </NButton>
        </template>
        <NButton v-else size="tiny" type="primary" :loading="busy === 'paper'" @click="pickPaper()">
          选择纸张
        </NButton>
      </div>
    </div>
    <div v-else-if="autoFont" class="quick-start-font-note">
      已自动使用系统字体「{{ store.systemFonts.find((f) => f.path === store.fontPath)?.name || store.fontPath }}」，
      想更像手写可在「字迹」里换成自己的字体。
    </div>
    </div>
  </div>
</template>

<style scoped>
.quick-start {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  pointer-events: none;
}

.quick-start-card {
  width: min(680px, 94%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 26px 30px 22px;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--card-bg);
  box-shadow: 0 18px 50px rgba(36, 49, 46, 0.16);
  text-align: center;
  pointer-events: auto;
  user-select: none;
}

html.dark .quick-start-card {
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
}

.quick-start-kicker {
  font-size: 11px;
  letter-spacing: 0.24em;
  text-transform: uppercase;
  color: var(--accent);
  opacity: 0.85;
}

.quick-start-title {
  margin: 0;
  font-family: var(--kaiti);
  font-size: 26px;
  font-weight: 700;
  color: var(--text-main);
}

.quick-start-sub {
  margin: 0 0 10px;
  font-size: 13px;
  color: var(--text-sub);
}

.quick-start-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
  max-width: 640px;
}

.quick-start-actions :deep(.n-button) {
  min-width: 150px;
}

.quick-start-main {
  font-weight: 700;
}

.quick-start-secondary {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 6px;
}

.quick-start-hint {
  font-size: 11.5px;
  color: var(--text-sub);
}

.quick-start-warn {
  margin-top: 14px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border: 1px solid rgba(229, 72, 77, 0.35);
  border-radius: 8px;
  background: rgba(229, 72, 77, 0.06);
  font-size: 12px;
  color: var(--text-main);
}

.quick-start-warn-actions {
  display: flex;
  gap: 8px;
}

.quick-start-font-note {
  margin-top: 12px;
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--text-sub);
  max-width: 520px;
}
</style>
