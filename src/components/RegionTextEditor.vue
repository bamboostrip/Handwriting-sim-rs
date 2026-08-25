<script setup lang="ts">
//! 区域文字编辑器（对话框内）：与主界面「待处理文本」（ParaEditor）一致的富文本多段模型——
//! 每行一个 contenteditable（plaintext-only），支持每行独立的对齐（左/中/右）与首行缩进，
//! 回车分行、段首退格并回上行、粘贴多行自动拆分。
//!
//! 数据模型：paragraphs: UiParagraph[] ({ text, align, indentEm })
//! 内部维护带稳定 id 的行列表，对齐/缩进即刻通过 CSS 生效，光标移动上抛当前行索引。

import { computed, nextTick, onMounted, ref, watch } from "vue";
import { cleanText } from "../store";
import type { UiParagraph } from "../types";

export interface RegionParaRow {
  id: number;
  text: string;
  align: 0 | 1 | 2;
  indentEm: number;
}

const props = withDefaults(
  defineProps<{
    /** 多段排版数据 */
    paragraphs: UiParagraph[];
    /** 当前聚焦行索引（0 基） */
    curIndex?: number;
    placeholder?: string;
  }>(),
  { curIndex: 0, placeholder: "" },
);

const emit = defineEmits<{
  "update:paragraphs": [value: UiParagraph[]];
  "update:curIndex": [value: number];
}>();

const ALIGN_CSS = ["left", "center", "right"] as const;
/** 与主界面 ParaEditor 一致的缩进像素基准（13px 字号） */
const INDENT_PX_PER_EM = 13;

let rowSeq = 1;
function toRow(p: UiParagraph): RegionParaRow {
  return {
    id: rowSeq++,
    text: p.text ?? "",
    align: (p.align as 0 | 1 | 2) ?? 0,
    indentEm: p.indentEm ?? 0,
  };
}

function initRows(list: UiParagraph[]): RegionParaRow[] {
  if (!list || list.length === 0) {
    return [{ id: rowSeq++, text: "", align: 0, indentEm: 0 }];
  }
  return list.map(toRow);
}

const rows = ref<RegionParaRow[]>(initRows(props.paragraphs));

const isEmpty = computed(
  () => rows.value.length === 1 && rows.value[0]?.text.trim() === "",
);

// ---- 行元素注册 ----
const rowEls = new Map<number, HTMLElement>();
const setRowEl = (id: number) => (el: unknown) => {
  if (el instanceof HTMLElement) rowEls.set(id, el);
  else rowEls.delete(id);
};

function rowStyle(p: RegionParaRow): Record<string, string> {
  return {
    textAlign: ALIGN_CSS[p.align] ?? "left",
    textIndent: p.indentEm > 0 ? `${p.indentEm * INDENT_PX_PER_EM}px` : "0",
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

/** 将当前 rows 状态同步回各行 DOM 节点的 innerText */
function syncDom(): void {
  for (let i = 0; i < rows.value.length; i++) {
    const row = rows.value[i];
    const el = rowEls.get(row.id);
    if (!el) continue;
    const shown = el.innerText.replace(/\n/g, "");
    if (shown !== row.text) {
      el.innerText = row.text;
    }
  }
}

/** 结构性修改后：重写各行 DOM 文本并把光标放到位 */
async function resync(focusRowId: number, caretAt: number): Promise<void> {
  await nextTick();
  syncDom();
  const target = rowEls.get(focusRowId);
  if (target) setCaret(target, caretAt);
}

function commit(): void {
  emit(
    "update:paragraphs",
    rows.value.map((r) => ({
      text: r.text,
      align: r.align,
      indentEm: r.indentEm,
    })),
  );
}

function onInput(r: RegionParaRow, e: Event): void {
  const el = e.target as HTMLElement;
  // Enter 已被拦截；防御性去掉可能混入的换行
  r.text = el.innerText.replace(/\n/g, "");
  commit();
}

function onFocusRow(i: number): void {
  emit("update:curIndex", i);
}

function onKeydown(i: number, e: KeyboardEvent): void {
  const el = e.currentTarget as HTMLElement;
  const cur = rows.value[i];
  if (!cur) return;
  if (e.key === "Enter" && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    const pos = Math.max(0, Math.min(caretOffset(el), cpLen(cur.text)));
    const chars = [...cur.text];
    const before = chars.slice(0, pos).join("");
    const after = chars.slice(pos).join("");
    cur.text = before;
    const nextRow: RegionParaRow = {
      id: rowSeq++,
      text: after,
      align: cur.align,
      indentEm: cur.indentEm,
    };
    rows.value.splice(i + 1, 0, nextRow);
    emit("update:curIndex", i + 1);
    commit();
    void resync(nextRow.id, 0);
  } else if (e.key === "Backspace" && caretCollapsedAtStart(el)) {
    e.preventDefault();
    if (i === 0) return;
    const prev = rows.value[i - 1];
    const joinedLen = cpLen(prev.text);
    prev.text = prev.text + cur.text;
    rows.value.splice(i, 1);
    emit("update:curIndex", i - 1);
    commit();
    void resync(prev.id, joinedLen);
  }
}

function onPaste(i: number, e: ClipboardEvent): void {
  const text = e.clipboardData?.getData("text/plain") ?? "";
  if (!text) return;
  e.preventDefault();
  const parts = text.split(/\r?\n/).map((s) => cleanText(s));
  if (parts.length <= 1) {
    document.execCommand("insertText", false, parts[0]);
    return;
  }
  const cur = rows.value[i];
  if (!cur) return;
  const el = e.currentTarget as HTMLElement;
  const off = Math.max(0, Math.min(caretOffset(el), cpLen(cur.text)));
  const chars = [...cur.text];
  const head = chars.slice(0, off).join("");
  const tail = chars.slice(off).join("");
  cur.text = head + parts[0];

  const newRows: RegionParaRow[] = [];
  for (let k = 1; k < parts.length; k++) {
    const isLast = k === parts.length - 1;
    newRows.push({
      id: rowSeq++,
      text: parts[k] + (isLast ? tail : ""),
      align: cur.align,
      indentEm: cur.indentEm,
    });
  }
  rows.value.splice(i + 1, 0, ...newRows);
  const lastTarget = newRows[newRows.length - 1];
  emit("update:curIndex", i + newRows.length);
  commit();
  void resync(lastTarget.id, cpLen(tail));
}

onMounted(() => {
  void nextTick(() => {
    syncDom();
  });
});

/** 外部结构性修改（如父组件修改对齐/缩进、导入 docx 或切换草稿） */
watch(
  () => props.paragraphs,
  async (newParas) => {
    if (!newParas || newParas.length === 0) return;
    const sameLength = newParas.length === rows.value.length;
    let textChanged = !sameLength;
    if (sameLength) {
      for (let k = 0; k < newParas.length; k++) {
        if (newParas[k].text !== rows.value[k].text) {
          textChanged = true;
          break;
        }
      }
    }
    if (textChanged) {
      rows.value = newParas.map((p) => toRow(p));
      await nextTick();
      syncDom();
    } else {
      // 仅 align / indentEm 变动，直接更新行属性（CSS 自动响应）
      for (let k = 0; k < newParas.length; k++) {
        rows.value[k].align = (newParas[k].align as 0 | 1 | 2) ?? 0;
        rows.value[k].indentEm = newParas[k].indentEm ?? 0;
      }
    }
  },
  { deep: true },
);
</script>

<template>
  <div class="rte" :class="{ 'is-empty': isEmpty }">
    <div
      v-for="(p, i) in rows"
      :key="p.id"
      class="rte-row"
      contenteditable="plaintext-only"
      spellcheck="false"
      :ref="setRowEl(p.id)"
      :style="rowStyle(p)"
      @input="onInput(p, $event)"
      @keydown="onKeydown(i, $event)"
      @paste="onPaste(i, $event)"
      @focusin="onFocusRow(i)"
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
