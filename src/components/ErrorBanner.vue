<script setup lang="ts">
//! 可恢复错误横幅：把状态栏里的一行字，变成「看得见 + 点得动」的下一步。
//!
//! 字体缺失 → 直接用系统字体 / 重新选择；背景缺失 → 重新选择；
//! 其他错误 → 打开教程或关闭。

import { NButton } from "naive-ui";
import {
  chooseBackground,
  chooseFont,
  doRender,
  ensureUsableFont,
  openHelpModal,
  store,
} from "../store";

async function useSystemFont(): Promise<void> {
  await ensureUsableFont();
  await doRender();
}

async function pickFont(): Promise<void> {
  await chooseFont();
  if (store.fontPath.trim()) await doRender();
}

async function pickBackground(): Promise<void> {
  await chooseBackground();
  if (store.backgroundPath.trim()) await doRender();
}

function close(): void {
  store.errorBanner = "";
  store.errorKind = "";
  store.errorSource = "";
}
</script>

<template>
  <div v-if="store.errorBanner" class="error-banner">
    <span class="error-banner-icon">⚠️</span>
    <span class="error-banner-text" :title="store.errorBanner">{{ store.errorBanner }}</span>
    <div class="error-banner-actions">
      <template v-if="store.errorKind === 'font'">
        <NButton size="tiny" type="primary" @click="useSystemFont()">使用系统字体</NButton>
        <NButton size="tiny" @click="pickFont()">选择字体</NButton>
      </template>
      <template v-else-if="store.errorKind === 'background'">
        <NButton size="tiny" type="primary" @click="pickBackground()">选择纸张</NButton>
      </template>
      <template v-else>
        <NButton size="tiny" @click="openHelpModal('quickstart')">打开教程</NButton>
      </template>
      <NButton size="tiny" quaternary @click="close()">关闭</NButton>
    </div>
  </div>
</template>

<style scoped>
.error-banner {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid rgba(229, 72, 77, 0.35);
  border-radius: 7px;
  background: rgba(229, 72, 77, 0.07);
  color: var(--text-main);
  font-size: 12px;
  line-height: 1.4;
  flex-shrink: 0;
}

.error-banner-icon {
  flex-shrink: 0;
}

.error-banner-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.error-banner-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
</style>
