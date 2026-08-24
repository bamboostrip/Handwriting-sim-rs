<script setup lang="ts">
//! 逐段编辑器：每段一个 contenteditable（plaintext-only）。
//!
//! 对齐原 Slint/Python 版交互：
//! - 回车（无修饰键、光标未选区）→ 当前段在光标处拆分，后半段继承格式
//! - 段首退格 → 并入上一段
//! - 粘贴多行 → 按行拆成多段（继承当前段对齐/缩进）
//! - 对齐方式即刻可见；首行缩进用 CSS text-indent 可视化（仅编辑区示意）
//! - 底部空白点击聚焦最后一段
//!
//! Web 实现细节：段落 DOM 不受控（输入不重渲染），仅在外部结构性修改
//! （导入 docx / 分段合并 / 预设）后按 id 同步文本。

import { nextTick, watch } from "vue";

import {
  cleanText,
  focusEmptyArea,
  mergePrev,
  pasteMulti,
  pendingFocus,
  splitPara,
  store,
  updateParaStatus,
} from "../store";
import type { Para } from "../store";

const ALIGN_CSS = ["left", "center", "right"] as const;

const rowEls = new Map<number, HTMLElement>();
const setRowEl = (id: number) => (el: unknown) => {
  if (el instanceof HTMLElement) rowEls.set(id, el);
  else rowEls.delete(id);
};

// ---- 光标工具（以 Unicode 码点计数，与 store 的 [...text] 拆分一致）----
const cpLen = (s: string): number => [...s].length;

function caretOffset(el: HTMLElement): number {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return 0;
  const range = sel.getRangeAt(0);
  if (!el.contains(range.startContainer)) return 0;
  const pre = range.cloneRange();
  pre.selectNodeContents(el);
  try {
    pre.setEnd(range.startContainer, range.startOffset);
  } catch {
    return 0;
  }
  return cpLen(pre.toString());
}

function caretCollapsedAtStart(el: HTMLElement): boolean {
  const sel = window.getSelection();
  if (!sel || !sel.isCollapsed || sel.rangeCount === 0) return false;
  const range = sel.getRangeAt(0);
  return el.contains(range.startContainer) && caretOffset(el) === 0;
}

function setCaret(el: HTMLElement, offset: number): void {
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  let remaining = Math.max(0, offset);
  let last: Text | null = null;
  let node = walker.nextNode() as Text | null;
  while (node) {
    const len = cpLen(node.data);
    if (remaining <= len) {
      let u16 = 0;
      const chars = [...node.data];
      for (let i = 0; i < remaining && i < chars.length; i++) u16 += chars[i].length;
      const sel = window.getSelection();
      const rng = document.createRange();
      rng.setStart(node, u16);
      rng.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(rng);
      return;
    }
    remaining -= len;
    last = node;
    node = walker.nextNode() as Text | null;
  }
  const sel = window.getSelection();
  const rng = document.createRange();
  if (last) rng.setStart(last, last.data.length);
  else rng.selectNodeContents(el);
  rng.collapse(true);
  sel?.removeAllRanges();
  sel?.addRange(rng);
}

/** 结构性修改后同步各行文本（导入 docx / 分段 / 合并 / 粘贴 / 初始挂载）。
 *  必须先于焦点 watcher 注册：先重写文本，再由焦点请求放置光标。
 *  纯打字路径不会触发重写（onInput 已把 p.text 同步为 DOM 内容）。 */
watch(
  () => store.paragraphs.map((p) => `${p.id}:${p.text}`).join("\u0001"),
  () => {
    void nextTick(() => {
      for (const p of store.paragraphs) {
        const el = rowEls.get(p.id);
        if (!el) continue;
        const shown = el.innerText.replace(/\n/g, "");
        if (shown !== p.text) el.innerText = p.text;
      }
    });
  },
  { immediate: true },
);

/** 外部焦点请求（分段/合并/导入后） */
watch(
  () => pendingFocus.nonce,
  () => {
    void nextTick(() => {
      const el = rowEls.get(pendingFocus.id);
      if (!el) return;
      setCaret(el, pendingFocus.offset);
      el.scrollIntoView({ block: "nearest" });
    });
  },
);

function onInput(p: Para, e: Event): void {
  const el = e.target as HTMLElement;
  // Enter 已被拦截；防御性去掉可能混入的换行
  p.text = el.innerText.replace(/\n/g, "");
  updateParaStatus();
}

function onKeydown(p: Para, e: KeyboardEvent): void {
  const el = e.currentTarget as HTMLElement;
  if (e.key === "Enter" && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    splitPara(p.id, caretOffset(el));
  } else if (e.key === "Backspace" && caretCollapsedAtStart(el)) {
    e.preventDefault();
    mergePrev(p.id);
  }
}

function onPaste(p: Para, e: ClipboardEvent): void {
  const text = e.clipboardData?.getData("text/plain") ?? "";
  if (!text) return;
  e.preventDefault();
  const el = e.currentTarget as HTMLElement;
  if (/\r?\n/.test(text)) {
    pasteMulti(p.id, caretOffset(el), text);
  } else {
    document.execCommand("insertText", false, text); // Chromium 支持，保留撤销栈
  }
}

function onFocusRow(p: Para): void {
  store.curParaId = p.id;
  updateParaStatus();
}

function isEmpty(): boolean {
  return (
    store.paragraphs.length === 1 && cleanText(store.paragraphs[0]?.text ?? "").trim() === ""
  );
}
</script>

<template>
  <div class="para-editor" :class="{ 'is-empty': isEmpty() }">
    <div
      v-for="p in store.paragraphs"
      :key="p.id"
      class="para-row"
      contenteditable="plaintext-only"
      spellcheck="false"
      :ref="setRowEl(p.id)"
      :style="{
        textAlign: ALIGN_CSS[p.align],
        textIndent: p.indentEm > 0 ? `${p.indentEm * 13}px` : '0',
      }"
      @input="onInput(p, $event)"
      @keydown="onKeydown(p, $event)"
      @paste="onPaste(p, $event)"
      @focusin="onFocusRow(p)"
    ></div>
    <div style="min-height: 30px" @click="focusEmptyArea()"></div>
  </div>
</template>
