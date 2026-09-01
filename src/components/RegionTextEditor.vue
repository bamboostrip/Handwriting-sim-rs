<script setup lang="ts">
//! 区域文字编辑器（对话框内）：与主界面「待处理文本」（ParaEditor）一致的富文本多段模型——
//! 支持跨段多选、批量删除、Ctrl+A 全选，并保留各段对齐（左/中/右）与首行缩进。

import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
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
const editorRef = ref<HTMLDivElement | null>(null);
let isInternalSync = false;

const isEmpty = computed(
  () => rows.value.length === 1 && rows.value[0]?.text.trim() === "",
);

function syncStoreToDom(force = false): void {
  if (!editorRef.value || (isInternalSync && !force)) return;
  const editor = editorRef.value;

  const domRows = Array.from(editor.querySelectorAll<HTMLElement>(":scope > .rte-row"));
  const domTexts = domRows.map((r) => r.innerText.replace(/\r?\n$/, ""));
  const storeTexts = rows.value.map((p) => p.text);

  const isSame =
    domRows.length === rows.value.length &&
    domTexts.every((t, i) => t === storeTexts[i]);

  if (isSame && !force) {
    domRows.forEach((r, i) => {
      const p = rows.value[i];
      if (p) {
        r.dataset.id = String(p.id);
        r.style.textAlign = ALIGN_CSS[p.align];
        r.style.textIndent = p.indentEm > 0 ? `${p.indentEm * INDENT_PX_PER_EM}px` : "0";
      }
    });
    return;
  }

  editor.innerHTML = "";
  for (const p of rows.value) {
    const row = document.createElement("div");
    row.className = "rte-row";
    row.dataset.id = String(p.id);
    row.style.textAlign = ALIGN_CSS[p.align];
    row.style.textIndent = p.indentEm > 0 ? `${p.indentEm * INDENT_PX_PER_EM}px` : "0";
    if (p.text) {
      row.textContent = p.text;
    } else {
      row.innerHTML = "<br>";
    }
    editor.appendChild(row);
  }
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

function syncDomToStore(): void {
  if (!editorRef.value) return;
  isInternalSync = true;
  const editor = editorRef.value;

  const childNodes = Array.from(editor.childNodes);
  const domRows: HTMLElement[] = [];

  for (const node of childNodes) {
    if (node instanceof HTMLElement && node.classList.contains("rte-row")) {
      domRows.push(node);
    } else if (node instanceof HTMLElement && node.tagName === "DIV") {
      node.classList.add("rte-row");
      domRows.push(node);
    } else if (node.nodeType === Node.TEXT_NODE && node.textContent?.trim()) {
      const wrapper = document.createElement("div");
      wrapper.className = "rte-row";
      wrapper.textContent = node.textContent;
      editor.replaceChild(wrapper, node);
      domRows.push(wrapper);
    }
  }

  if (domRows.length === 0) {
    const emptyRow = document.createElement("div");
    emptyRow.className = "rte-row";
    emptyRow.innerHTML = "<br>";
    const np: RegionParaRow = { id: rowSeq++, text: "", align: 0, indentEm: 0 };
    emptyRow.dataset.id = String(np.id);
    editor.appendChild(emptyRow);
    rows.value = [np];
    commit();
    isInternalSync = false;
    return;
  }

  const newRows: RegionParaRow[] = [];
  for (let i = 0; i < domRows.length; i++) {
    const el = domRows[i];
    let rawText = el.innerText.replace(/\r?\n$/, "");
    if (rawText === "\n") rawText = "";

    const existingId = Number(el.dataset?.id);
    const existingRow = rows.value.find((p) => p.id === existingId);

    const align = existingRow ? existingRow.align : 0;
    const indentEm = existingRow ? existingRow.indentEm : 0;

    el.style.textAlign = ALIGN_CSS[align];
    el.style.textIndent = indentEm > 0 ? `${indentEm * INDENT_PX_PER_EM}px` : "0";

    const r: RegionParaRow = existingRow
      ? { ...existingRow, text: rawText }
      : { id: existingId || rowSeq++, text: rawText, align, indentEm };

    el.dataset.id = String(r.id);
    newRows.push(r);
  }

  rows.value = newRows;
  commit();
  updateCurrentRowIndexFromSelection();
  isInternalSync = false;
}

function updateCurrentRowIndexFromSelection(): void {
  const sel = window.getSelection();
  if (!sel || !sel.anchorNode || !editorRef.value) return;
  let node: Node | null = sel.anchorNode;
  while (node && node !== editorRef.value) {
    if (node instanceof HTMLElement && node.classList.contains("rte-row")) {
      const id = Number(node.dataset.id);
      const idx = rows.value.findIndex((r) => r.id === id);
      if (idx >= 0) {
        emit("update:curIndex", idx);
        return;
      }
    }
    node = node.parentNode;
  }
}

function onInput(): void {
  syncDomToStore();
}

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || !editorRef.value) return;
    const range = sel.getRangeAt(0);
    range.deleteContents();

    let curRow: HTMLElement | null = null;
    let node: Node | null = range.startContainer;
    while (node && node !== editorRef.value) {
      if (node instanceof HTMLElement && node.classList.contains("rte-row")) {
        curRow = node;
        break;
      }
      node = node.parentNode;
    }

    const newRow = document.createElement("div");
    newRow.className = "rte-row";
    newRow.dataset.id = String(rowSeq++);

    if (curRow) {
      const subRange = document.createRange();
      subRange.setStart(range.startContainer, range.startOffset);
      subRange.setEndAfter(curRow.lastChild || curRow);
      const extracted = subRange.extractContents();
      newRow.appendChild(extracted);
      if (!newRow.textContent) newRow.innerHTML = "<br>";
      if (!curRow.textContent) curRow.innerHTML = "<br>";

      newRow.style.textAlign = curRow.style.textAlign;
      newRow.style.textIndent = curRow.style.textIndent;
      curRow.after(newRow);
    } else {
      newRow.innerHTML = "<br>";
      editorRef.value.appendChild(newRow);
    }

    const newRange = document.createRange();
    newRange.setStart(newRow, 0);
    newRange.collapse(true);
    sel.removeAllRanges();
    sel.addRange(newRange);

    syncDomToStore();
  }
}

function onPaste(e: ClipboardEvent): void {
  const text = e.clipboardData?.getData("text/plain");
  if (!text) return;
  e.preventDefault();

  const lines = text.split(/\r?\n/).map((s) => cleanText(s));
  if (lines.length === 1) {
    document.execCommand("insertText", false, lines[0]);
    syncDomToStore();
    return;
  }

  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0 || !editorRef.value) return;
  const range = sel.getRangeAt(0);
  range.deleteContents();

  let curRow: HTMLElement | null = null;
  let node: Node | null = range.startContainer;
  while (node && node !== editorRef.value) {
    if (node instanceof HTMLElement && node.classList.contains("rte-row")) {
      curRow = node;
      break;
    }
    node = node.parentNode;
  }

  const align = curRow ? curRow.style.textAlign : "left";
  const indent = curRow ? curRow.style.textIndent : "0";

  if (curRow) {
    const textNode = document.createTextNode(lines[0]);
    range.insertNode(textNode);
    range.setStartAfter(textNode);
    range.collapse(true);
  }

  let insertAfter = curRow;
  for (let i = curRow ? 1 : 0; i < lines.length; i++) {
    const newRow = document.createElement("div");
    newRow.className = "rte-row";
    newRow.dataset.id = String(rowSeq++);
    newRow.style.textAlign = align;
    newRow.style.textIndent = indent;
    newRow.textContent = lines[i] || "";
    if (!lines[i]) newRow.innerHTML = "<br>";

    if (insertAfter && insertAfter.parentNode) {
      insertAfter.after(newRow);
      insertAfter = newRow;
    } else {
      editorRef.value.appendChild(newRow);
      insertAfter = newRow;
    }
  }

  if (insertAfter) {
    const newRange = document.createRange();
    newRange.selectNodeContents(insertAfter);
    newRange.collapse(false);
    sel.removeAllRanges();
    sel.addRange(newRange);
  }

  syncDomToStore();
}

function onSelectionChange(): void {
  if (
    document.activeElement === editorRef.value ||
    editorRef.value?.contains(document.activeElement)
  ) {
    updateCurrentRowIndexFromSelection();
  }
}

onMounted(() => {
  document.addEventListener("selectionchange", onSelectionChange);
  syncStoreToDom(true);
});

onBeforeUnmount(() => {
  document.removeEventListener("selectionchange", onSelectionChange);
});

/** 外部结构性修改（如父组件修改对齐/缩进） */
watch(
  () => props.paragraphs,
  (newParas) => {
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
      void nextTick(() => syncStoreToDom(true));
    } else {
      for (let k = 0; k < newParas.length; k++) {
        rows.value[k].align = (newParas[k].align as 0 | 1 | 2) ?? 0;
        rows.value[k].indentEm = newParas[k].indentEm ?? 0;
      }
      void nextTick(() => syncStoreToDom(false));
    }
  },
  { deep: true },
);
</script>

<template>
  <div
    ref="editorRef"
    class="rte"
    :class="{ 'is-empty': isEmpty }"
    contenteditable="true"
    spellcheck="false"
    @input="onInput()"
    @keydown="onKeydown($event)"
    @paste="onPaste($event)"
  >
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
  background: var(--input-bg, #fff);
  padding: 4px 10px 4px 8px;
  cursor: text;
  outline: none;
}

.rte:focus,
.rte:focus-within {
  border-color: var(--accent);
}

.rte-row {
  font-family: var(--kaiti);
  font-size: 13px;
  line-height: 1.55;
  min-height: 20px;
  outline: none;
  white-space: pre-wrap;
  word-break: break-all;
  border-bottom: 1px solid var(--row-border, #eef2ee);
  padding: 0 2px 1px;
}

.rte-row:focus {
  background: var(--hover-bg, #fbfdfb);
}

.rte-placeholder {
  position: absolute;
  top: 6px;
  left: 12px;
  color: var(--placeholder-color, #9aa8a4);
  font-size: 12px;
  pointer-events: none;
}
</style>

