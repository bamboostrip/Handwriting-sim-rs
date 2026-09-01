<script setup lang="ts">
//! 发现新版本与自动更新提示对话框（对齐 Python 原版 update_dialog.py）。
//!
//! 支持版本对比、更新日志 Markdown 渲染、跳过版本、浏览器下载
//! 以及便携版后台一键自动下载覆盖并平滑重启。

import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  NButton,
  NCheckbox,
  NModal,
  NProgress,
  NTag,
  useDialog,
  useMessage,
} from "naive-ui";

import { api } from "../api";
import {
  APP_VERSION,
  closeUpdateModal,
  isDarkActive,
  openExternalUrl,
  setSkippedVersion,
  store,
} from "../store";


const dialog = useDialog();
const message = useMessage();

const skipThisVersion = ref(false);
const downloading = ref(false);
const downloadPercent = ref(0);
const downloadStatusText = ref("");
let unlistenProgress: (() => void) | null = null;

const info = computed(() => store.updateInfo);

onMounted(async () => {
  try {
    unlistenProgress = await api.onDownloadProgress((payload) => {
      downloadPercent.value = Math.min(100, Math.max(0, Math.round(payload.percent)));
      const recvMb = (payload.received / (1024 * 1024)).toFixed(1);
      const totalMb = (payload.total / (1024 * 1024)).toFixed(1);
      if (payload.total > 0) {
        downloadStatusText.value = `正在下载更新包：${recvMb} MB / ${totalMb} MB (${downloadPercent.value}%)`;
      } else {
        downloadStatusText.value = `正在下载更新包：${recvMb} MB`;
      }
    });
  } catch (e) {
    console.error("注册下载进度监听失败:", e);
  }
});

onBeforeUnmount(() => {
  if (unlistenProgress) {
    unlistenProgress();
    unlistenProgress = null;
  }
});

function handleClose(): void {
  if (skipThisVersion.value && info.value?.version) {
    setSkippedVersion(info.value.version);
  }
  closeUpdateModal(skipThisVersion.value);
}

function openInBrowser(): void {
  const url = info.value?.htmlUrl || "https://github.com/bamboostrip/Handwriting-sim-rs/releases";
  openExternalUrl(url);
}

async function startAutoUpdate(): Promise<void> {
  const assetUrl = info.value?.assetUrl;
  if (!assetUrl) {
    // 若 release 中没有直接的 exe/zip 资产，退回浏览器手动下载
    openInBrowser();
    return;
  }

  downloading.value = true;
  downloadPercent.value = 0;
  downloadStatusText.value = "准备连接下载服务器…";

  try {
    const fileName = info.value?.assetName || `handwrite-sim-${info.value?.version || "new"}.exe`;
    const downloadedPath = await api.downloadUpdate(assetUrl, fileName);

    downloadStatusText.value = "✅ 下载完成，准备重启并应用新版本…";

    dialog.success({
      title: "更新下载完成",
      content: "新版本已准备就绪，点击「立即重启」后程序将自动覆盖并升级至新版本。",
      positiveText: "立即重启升级",
      closable: false,
      maskClosable: false,
      onPositiveClick: async () => {
        try {
          await api.applyPortableUpdate(downloadedPath);
        } catch (e) {
          message.error(`应用更新失败: ${e}`);
        }
      },
    });
  } catch (e) {
    downloadStatusText.value = `❌ 下载失败: ${e}`;
    dialog.error({
      title: "更新下载失败",
      content: `未能成功下载更新包（${e}）。\n您可以点击「浏览器下载」前往 GitHub 手动下载。`,
      positiveText: "前往浏览器下载",
      negativeText: "稍后重试",
      onPositiveClick: () => {
        openInBrowser();
      },
    });
  } finally {
    downloading.value = false;
  }
}
</script>

<template>
  <NModal
    v-model:show="store.updateModalOpen"
    preset="card"
    title="🎉 发现新版本"
    style="width: 540px; max-width: 92vw"
    :mask-closable="!downloading"
    :closable="!downloading"
    @update:show="(val: boolean) => !val && handleClose()"
  >
    <div class="update-modal-body" v-if="info">
      <!-- 头部：版本对比 -->
      <div class="version-banner">
        <div class="version-badge-row">
          <span class="version-label">当前版本：</span>
          <NTag size="small" type="default">v{{ APP_VERSION }}</NTag>
          <span class="version-arrow">➔</span>
          <span class="version-label">最新版本：</span>
          <NTag size="small" type="success">v{{ info.version }}</NTag>
        </div>
        <div class="release-title">{{ info.title || `v${info.version}` }}</div>
      </div>

      <!-- 更新日志展示区 -->
      <div class="changelog-section">
        <div class="changelog-heading">更新内容：</div>
        <div class="changelog-content">
          <pre class="changelog-text">{{ info.body }}</pre>
        </div>
      </div>

      <!-- 跳过此版本 -->
      <div class="skip-row" v-if="!downloading">
        <NCheckbox v-model:checked="skipThisVersion">
          跳过此版本（不再自动提醒 v{{ info.version }}）
        </NCheckbox>
      </div>

      <!-- 下载进度展示区 -->
      <div class="download-progress-area" v-if="downloading || downloadStatusText">
        <div class="progress-status-text">{{ downloadStatusText }}</div>
        <NProgress
          type="line"
          :percentage="downloadPercent"
          :indicator-placement="'inside'"
          processing
          :color="isDarkActive() ? '#5ea84d' : '#2e7d74'"
        />
      </div>

    </div>

    <template #footer>
      <div class="modal-footer-row">
        <NButton
          secondary
          size="medium"
          :disabled="downloading"
          @click="openInBrowser()"
        >
          🌐 浏览器下载
        </NButton>

        <div style="flex: 1" />

        <NButton
          size="medium"
          :disabled="downloading"
          @click="handleClose()"
        >
          稍后提醒
        </NButton>

        <NButton
          type="primary"
          size="medium"
          :loading="downloading"
          @click="startAutoUpdate()"
        >
          🚀 立即自动更新
        </NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.update-modal-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.version-banner {
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--accent-soft, #f4f8f7);
  border: 1px solid var(--border, #dce8e5);
  border-radius: 6px;
  padding: 10px 12px;
}

.version-badge-row {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.version-label {
  color: var(--text-sub, #6b7a76);
  font-size: 12px;
}

.version-arrow {
  color: var(--accent, #2e7d74);
  font-weight: bold;
  padding: 0 4px;
}

.release-title {
  font-size: 13.5px;
  font-weight: 700;
  color: var(--text-main, #24312e);
}

.changelog-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.changelog-heading {
  font-size: 12.5px;
  font-weight: 700;
  color: var(--text-main, #24312e);
}

.changelog-content {
  max-height: 180px;
  min-height: 90px;
  overflow-y: auto;
  background: var(--card-bg, #ffffff);
  border: 1px solid var(--border, #d8e2df);
  border-radius: 6px;
  padding: 10px 12px;
}

.changelog-text {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-main, #24312e);
}

.skip-row {
  font-size: 12px;
}

.download-progress-area {
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--card-bg, #fbfdfc);
  border: 1px solid var(--border, #e1edea);
  border-radius: 6px;
  padding: 10px 12px;
}


.progress-status-text {
  font-size: 12px;
  color: var(--text-main, #24312e);
}

.modal-footer-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
}
</style>
