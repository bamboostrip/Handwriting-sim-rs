<script setup lang="ts">
//! 区域文字编辑器（对话框内）：与主界面「待处理文本」一致的富文本观感——
//! 每行一个 contenteditable（plaintext-only），对齐 / 首行缩进即刻可见，
//! 回车分行、段首退格并回上行、粘贴多行自动拆分。
//!
//! 与 ParaEditor 的差异：数据模型是单个多行字符串（draft.text，\n 分段），
//! 对齐 / 缩进为区域级统一设置；组件内部维护行数组，输入即上抛拼接文本，
//! 外部结构性修改（导入 docx）经 props.text 变化整树同步。

import { computed, nextTick, ref, watch } from "vue";
import { cleanText } from "../store";

const props = withDefaults(
  defineProps<{
    /** 多行文本（\n 分段） */
    text: string;
    /** 0 左对齐 / 1 居中 / 2 右对齐 */
    align?: number;
    /** 首行缩进（字符数 em），按 13px/字 可视化 */
    indentEm?: number;
    placeholder?: string;
  }>(),
  { align: 0, indentEm: 0, placeholder: "" },
);

const emit = defineEmits<{ "update:text": [value: string] }>();

const ALIGN_CSS = ["left", "center", "right"] as const;
/** 与主界面 ParaEditor 一致的缩进像素基准（13px 字号） */
const INDENT_PX_PER_EM = 13;

const lines = ref<string[]>(props.text === "" ? [""] : props.text.split("\n"));

const isEmpty = computed(() => lines.value.length === 1 && lines.value[0] === "");

// ---- 行元素注册 ----
const rowEls = new Map<number, HTMLElement>();
const setRowEl = (i: number) => (el: unknown) => {
  if (el instanceof HTMLElement) rowEls.set(i, el);
  else rowEls.delete(i);
};

function rowStyle(): Record<string, string> {
  return {
    textAlign: ALIGN_CSS[props.align] ?? "left",
    textIndent: props.indentEm > 0 ? `${props.indentEm * INDENT_PX_PER_EM}px` : "0",
  };
}

// ---- 光标工具（以 Unicode 码点计数）----
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
  el.focus();
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

/** 结构性修改后：重写各行 DOM 文本并把光标放到位 */
async function resync(focusRow: number, caretAt: number): Promise<void> {
  await nextTick();
  for (let i = 0; i < lines.value.length; i++) {
    const el = rowEls.get(i);
    if (!el) continue;
    const shown = el.innerText.replace(/\n/g, "");
    if (shown !== lines.value[i]) el.innerText = lines.value[i];
  }
  const target = rowEls.get(focusRow);
  if (target) setCaret(target, caretAt);
}

function commit(): void {
  emit("update:text", lines.value.join("\n"));
}

function onInput(i: number, e: Event): void {
  const el = e.target as HTMLElement;
  // Enter 已被拦截；防御性去掉可能混入的换行
  lines.value[i] = el.innerText.replace(/\n/g, "");
  commit();
}

function onKeydown(i: number, e: KeyboardEvent): void {
  const el = e.currentTarget as HTMLElement;
  if (e.key === "Enter" && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    const pos = Math.max(0, Math.min(caretOffset(el), cpLen(lines.value[i])));
    const chars = [...lines.value[i]];
    const before = chars.slice(0, pos).join("");
    const after = chars.slice(pos).join("");
    lines.value.splice(i, 1, before, after);
    commit();
    void resync(i + 1, 0);
  } else if (e.key === "Backspace" && caretCollapsedAtStart(el)) {
    e.preventDefault();
    if (i === 0) return;
    const joinedLen = cpLen(lines.value[i - 1]);
    lines.value.splice(i - 1, 2, lines.value[i - 1] + lines.value[i]);
    commit();
    void resync(i - 1, joinedLen);
  }
}

function onPaste(i: number, e: ClipboardEvent): void {
  const text = e.clipboardData?.getData("text/plain") ?? "";
  if (!text) return;
  e.preventDefault();
  const parts = text.split(/\r?\n/).map((s) => cleanText(s));
  if (parts.length <= 1) {
    document.execCommand("insertText", false, parts[0]); // Chromium 支持，保留撤销栈
    return;
  }
  const el = e.currentTarget as HTMLElement;
  const off = Math.max(0, Math.min(caretOffset(el), cpLen(lines.value[i])));
  const chars = [...lines.value[i]];
  const head = chars.slice(0, off).join("");
  const tail = chars.slice(off).join("");
  const inserted = [...parts];
  inserted[0] = head + inserted[0];
  inserted[inserted.length - 1] += tail;
  lines.value.splice(i, 1, ...inserted);
  commit();
  void resync(i + inserted.length - 1, cpLen(tail));
}

/** 外部结构性修改（导入 docx / 确定前清空）：按 id 同步文本 */
watch(
  () => props.text,
  (t) => {
    const joined = lines.value.join("\n");
    if (t === joined) return;
    lines.value = t === "" ? [""] : t.split("\n");
    void nextTick(() => {
      for (let i = 0; i < lines.value.length; i++) {
        const el = rowEls.get(i);
        if (!el) continue;
        const shown = el.innerText.replace(/\n/g, "");
        if (shown !== lines.value[i]) el.innerText = lines.value[i];
      }
    });
  },
);
</script>

<template>
  <div class="rte" :class="{ 'is-empty': isEmpty }">
    <div
      v-for="(_, i) in lines"
      :key="i"
      class="rte-row"
      contenteditable="plaintext-only"
      spellcheck="false"
      :ref="setRowEl(i)"
      :style="rowStyle()"
      @input="onInput(i, $event)"
      @keydown="onKeydown(i, $event)"
      @paste="onPaste(i, $event)"
    ></div>
    <div v-if="isEmpty && placeholder" class="rte-placeholder">{{ placeholder }}</div>
  </div>
</template>

<style scoped>
/* 观感对齐主界面 .para-editor / .para-row */
.rte {
  position: relative;
  min-height: 72px;
  max-height: 150px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: #fff;
  padding: 4px 10px 4px 8px;
  cursor: text;
}

.rte-row {
  font-family: var(--kaiti);
  font-size: 13px;
  line-height: 1.55;
  min-height: 20px;
  outline: none;
  white-space: pre-wrap;
  word-break: break-all;
  border-bottom: 1px solid #eef2ee;
  padding: 0 2px 1px;
}

.rte-row:focus {
  background: #fbfdfb;
}

.rte-placeholder {
  position: absolute;
  top: 6px;
  left: 12px;
  color: #9aa8a4;
  font-size: 12px;
  pointer-events: none;
}
</style>
