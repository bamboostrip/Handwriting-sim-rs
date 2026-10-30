<script setup lang="ts">
//! 首次启动引导：只问一个问题——你主要想用它做什么。
//!
//! 三个入口分别对应：写文字 / 填文档（高亮填空）/ 看示例。
//! 字体与纸张由 store 的兜底逻辑保证可用，因此这里不设置「必须先配置」的门槛。

import { ref } from "vue";
import { NButton, NModal, NSpin } from "naive-ui";
import {
  currentFontLabel,
  dismissOnboarding,
  handleOnboardingTask,
  store,
} from "../store";

const busy = ref<"" | "text" | "document" | "sample">("");

async function choose(task: "text" | "document" | "sample"): Promise<void> {
  if (busy.value) return;
  busy.value = task;
  try {
    await handleOnboardingTask(task);
  } finally {
    busy.value = "";
  }
}

function onShowChange(show: boolean): void {
  if (!show) dismissOnboarding();
}
</script>

<template>
  <NModal
    :show="store.onboardingOpen"
    preset="card"
    title="开始使用手写模拟器"
    style="width: 660px; max-width: 94vw"
    :mask-closable="false"
    :closable="true"
    @update:show="onShowChange"
  >
    <div class="onboarding-body">
      <p class="onboarding-lead">
        先选一个开始方式，剩下的交给软件。三步之内就能看到第一张手写成品。
      </p>

      <div class="onboarding-grid">
        <button
          type="button"
          class="onboarding-card"
          :disabled="!!busy"
          @click="choose('text')"
        >
          <span class="onboarding-icon">✍️</span>
          <span class="onboarding-card-title">写一段文字</span>
          <span class="onboarding-card-desc">
            信、作业、笔记。粘贴或输入文字，选字迹和纸张，直接导出图片 / PDF。
          </span>
        </button>

        <button
          type="button"
          class="onboarding-card"
          :disabled="!!busy"
          @click="choose('document')"
        >
          <span class="onboarding-icon">📑</span>
          <span class="onboarding-card-title">填试卷 / 表格</span>
          <span class="onboarding-card-desc">
            已有 Word / PDF 模板？高亮要手写的部分，导入后自动识别成手写填空区域。
          </span>
        </button>

        <button
          type="button"
          class="onboarding-card"
          :disabled="!!busy"
          @click="choose('sample')"
        >
          <span class="onboarding-icon">👀</span>
          <span class="onboarding-card-title">先看示例</span>
          <span class="onboarding-card-desc">
            30 秒载入一段示例文字和一套现成版式，先看看效果再决定怎么用。
          </span>
        </button>
      </div>

      <div class="onboarding-note">
        <span>当前字迹：{{ currentFontLabel() }}</span>
        <span class="onboarding-note-sep">·</span>
        <span>软件不内置商业字体，首次会自动选用系统楷体，之后可随时在「字迹」里更换。</span>
      </div>

      <div v-if="busy" class="onboarding-busy">
        <NSpin size="small" />
        <span>正在准备…</span>
      </div>
    </div>

    <template #footer>
      <div class="onboarding-footer">
        <NButton quaternary size="small" @click="dismissOnboarding()">
          跳过，直接进入
        </NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.onboarding-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.onboarding-lead {
  margin: 0;
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text-sub);
}

.onboarding-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
}

.onboarding-card {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 14px 14px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--card-bg);
  color: var(--text-main);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.15s, transform 0.15s, box-shadow 0.15s;
}

.onboarding-card:hover:not(:disabled) {
  border-color: var(--accent);
  transform: translateY(-1px);
  box-shadow: 0 6px 18px rgba(46, 125, 116, 0.12);
}

.onboarding-card:disabled {
  opacity: 0.6;
  cursor: default;
}

.onboarding-icon {
  font-size: 22px;
  line-height: 1;
}

.onboarding-card-title {
  font-size: 14px;
  font-weight: 700;
}

.onboarding-card-desc {
  font-size: 12px;
  line-height: 1.55;
  color: var(--text-sub);
}

.onboarding-note {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 9px 11px;
  border-radius: 8px;
  background: var(--hover-bg);
  color: var(--text-sub);
  font-size: 11.5px;
  line-height: 1.5;
}

.onboarding-note-sep {
  opacity: 0.5;
}

.onboarding-busy {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-sub);
  font-size: 12px;
}

.onboarding-footer {
  display: flex;
  justify-content: flex-end;
}
</style>
