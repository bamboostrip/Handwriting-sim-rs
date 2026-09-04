<script setup lang="ts">
//! 关于软件与开源项目展示对话框（对齐 Python 原版 about_dialog.py）。
//!
//! 包含软件基本信息、两个开源项目直达卡片（Python 版与 Rust 重构版）、
//! 自动更新设置以及手动联网检查更新功能。

import {
  NButton,
  NCheckbox,
  NModal,
  NRadioButton,
  NRadioGroup,
  NTag,
  NText,
} from "naive-ui";

import {
  APP_VERSION,
  PYTHON_REPO_URL,
  RUST_REPO_URL,
  closeAboutModal,
  manualCheckUpdate,
  openExternalUrl,
  openHelpModal,
  setAutoCheckUpdate,
  setThemePreference,
  store,
} from "../store";

</script>

<template>
  <NModal
    v-model:show="store.aboutModalOpen"
    preset="card"
    title="关于手写模拟器"
    style="width: 520px; max-width: 92vw"
    :mask-closable="true"
    @update:show="(val: boolean) => !val && closeAboutModal()"
  >
    <div class="about-container">
      <!-- 头部：应用信息与版本 -->
      <div class="about-header">
        <div class="about-logo">✍️</div>
        <div class="about-meta">
          <div class="about-title">
            <span>手写模拟器</span>
            <NTag type="success" size="small" round>v{{ APP_VERSION }}</NTag>
          </div>
          <div class="about-subtitle">Handwriting Simulator (Rust 极速重构版)</div>
        </div>
      </div>

      <!-- 简介说明 -->
      <div class="about-desc">
        基于自研笔画扰动渲染引擎的国风手写字迹模拟与排版生成工具。<br />
        支持背景底图模板、笔画高斯扰动、错字涂改、图文混排与多页 PDF 导出。
      </div>

      <!-- 使用指南引导卡片 -->
      <div class="about-help-bar">
        <div class="help-bar-info">
          <span class="help-bar-icon">📖</span>
          <span>快速上手、试卷高亮填空与全部功能使用教程</span>
        </div>
        <NButton size="small" type="primary" secondary @click="closeAboutModal(); openHelpModal()">
          使用指南
        </NButton>
      </div>

      <!-- 开源项目与重构版 -->
      <div class="about-section">
        <div class="section-heading">🌟 开源项目与源码仓库</div>
        <div class="repo-card-list">
          <div class="repo-card is-rust" @click="openExternalUrl(RUST_REPO_URL)">
            <div class="repo-info">
              <div class="repo-title">
                <span class="repo-icon">🦀</span>
                <strong>Rust 极速重构版（当前项目主仓库）</strong>
              </div>
              <div class="repo-desc">纯 Rust 引擎 + Tauri 2 架构，毫秒级预览与超低资源占用</div>
              <div class="repo-url">{{ RUST_REPO_URL }}</div>
            </div>
            <NButton size="tiny" type="primary">
              🚀 打开
            </NButton>
          </div>

          <div class="repo-card" @click="openExternalUrl(PYTHON_REPO_URL)">
            <div class="repo-info">
              <div class="repo-title">
                <span class="repo-icon">🐍</span>
                <strong>Python 原版项目仓库</strong>
              </div>
              <div class="repo-desc">基于 handright 核心的 Python 原版实现</div>
              <div class="repo-url">{{ PYTHON_REPO_URL }}</div>
            </div>
            <NButton size="tiny" secondary type="primary">
              🌐 打开
            </NButton>
          </div>
        </div>
      </div>


      <!-- 外观主题设置 -->
      <div class="about-section">
        <div class="section-heading">🎨 外观主题</div>
        <div class="update-box">
          <div style="display: flex; align-items: center; justify-content: space-between">
            <span style="font-size: 12.5px; color: var(--text-main)">界面色彩模式：</span>
            <NRadioGroup
              :value="store.themePreference"
              @update:value="(val: any) => setThemePreference(val)"
              size="small"
            >
              <NRadioButton value="auto">🖥️ 跟随系统</NRadioButton>
              <NRadioButton value="light">☀️ 浅色</NRadioButton>
              <NRadioButton value="dark">🌙 深色</NRadioButton>
            </NRadioGroup>
          </div>
        </div>
      </div>

      <!-- 版本更新设置与主动检查 -->
      <div class="about-section">
        <div class="section-heading">🔄 版本更新</div>
        <div class="update-box">
          <NCheckbox
            :checked="store.autoCheckUpdate"
            @update:checked="(v: boolean) => setAutoCheckUpdate(v)"
          >
            启动软件时自动检查更新
          </NCheckbox>

          <div class="update-action-row">
            <NText depth="3" class="update-status-text">
              {{ store.updateStatusText }}
            </NText>
            <NButton
              size="small"
              type="primary"
              ghost
              :loading="store.checkingUpdate"
              @click="manualCheckUpdate()"
            >
              🔄 检查更新
            </NButton>
          </div>
        </div>
      </div>
    </div>


    <template #footer>
      <div style="display: flex; justify-content: flex-end">
        <NButton type="primary" size="medium" @click="closeAboutModal()">
          确定
        </NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.about-container {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.about-header {
  display: flex;
  align-items: center;
  gap: 14px;
}

.about-logo {
  font-size: 38px;
  width: 52px;
  height: 52px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-soft, #e3efed);
  border-radius: 12px;
  border: 1px solid var(--border, #d8e2df);
  flex-shrink: 0;
}

.about-meta {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.about-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
  font-weight: 700;
  color: var(--text-main, #24312e);
}

.about-subtitle {
  font-size: 12px;
  color: var(--text-sub, #6b7a76);
}

.about-desc {
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-main, #24312e);
  background: var(--card-bg, #fbfdfc);
  border: 1px solid var(--border, #eef3f1);
  border-radius: 6px;
  padding: 8px 12px;
}

.about-help-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 9px 12px;
  background: var(--accent-soft, rgba(46, 125, 116, 0.08));
  border: 1px dashed var(--accent, #2e7d74);
  border-radius: 6px;
}

.help-bar-info {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text-main, #24312e);
  font-weight: 500;
}

.help-bar-icon {
  font-size: 16px;
}

.about-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.section-heading {
  font-size: 12.5px;
  font-weight: 700;
  color: var(--accent, #2e7d74);
}

.repo-card-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.repo-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 12px;
  background: var(--card-bg, #ffffff);
  border: 1px solid var(--border, #d8e2df);
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.repo-card:hover {
  border-color: var(--accent, #2e7d74);
  background: var(--hover-bg, #f6faf9);
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
}

.repo-card.is-rust {
  border-color: var(--accent, #2e7d74);
  background: var(--accent-soft, #fafdfc);
}

.repo-card.is-rust:hover {
  border-color: var(--accent-hover, #3d948a);
  background: var(--hover-bg, #f0f7f5);
}

.repo-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.repo-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-main, #24312e);
}

.repo-icon {
  font-size: 14px;
}

.repo-desc {
  font-size: 11.5px;
  color: var(--text-sub, #6b7a76);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.repo-url {
  font-size: 11px;
  color: var(--accent, #2e7d74);
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.update-box {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  background: var(--card-bg, #ffffff);
  border: 1px solid var(--border, #d8e2df);
  border-radius: 6px;
}


.update-action-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.update-status-text {
  font-size: 12px;
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
