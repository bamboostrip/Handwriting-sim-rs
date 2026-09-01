<script setup lang="ts">
//! 逐段文本编辑器：支持跨段多选、批量选中删除、Ctrl+A 全选，
//! 同时保留各段独立对齐（左/中/右）与首行缩进（textIndent）功能。

import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  cleanText,
  focusEmptyArea,
  newPara,
  pendingFocus,
  store,
  updateParaStatus,
} from "../store";
import type { Para } from "../store";

const ALIGN_CSS = ["left", "center", "right"] as const;

const editorRef = ref<HTMLDivElement | null>(null);
let isInternalSync = false;

function isEmpty(): boolean {
  return (
    store.paragraphs.length === 1 && cleanText(store.paragraphs[0]?.text ?? "").trim() === ""
  );
}

/** 将 store.paragraphs 完整同步到编辑器 DOM */
function syncStoreToDom(force = false): void {
  if (!editorRef.value || (isInternalSync && !force)) return;
  const editor = editorRef.value;

  const domRows = Array.from(editor.querySelectorAll<HTMLElement>(":scope > .para-row"));
  const domTexts = domRows.map((r) => r.innerText.replace(/\r?\n$/, ""));
  const storeTexts = store.paragraphs.map((p) => p.text);

  const isSame =
    domRows.length === store.paragraphs.length &&
    domTexts.every((t, i) => t === storeTexts[i]);

  if (isSame && !force) {
    domRows.forEach((r, i) => {
      const p = store.paragraphs[i];
      if (p) {
        r.dataset.id = String(p.id);
        r.style.textAlign = ALIGN_CSS[p.align];
        r.style.textIndent = p.indentEm > 0 ? `${p.indentEm * 13}px` : "0";
      }
    });
    return;
  }

  editor.innerHTML = "";

  for (const p of store.paragraphs) {
    const row = document.createElement("div");
    row.className = "para-row";
    row.dataset.id = String(p.id);
    row.style.textAlign = ALIGN_CSS[p.align];
    row.style.textIndent = p.indentEm > 0 ? `${p.indentEm * 13}px` : "0";
    if (p.text) {
      row.textContent = p.text;
    } else {
      row.innerHTML = "<br>";
    }
    editor.appendChild(row);
  }
}

/** 从当前 DOM 提取全部段落并更新 store.paragraphs */
function syncDomToStore(): void {
  if (!editorRef.value) return;
  isInternalSync = true;
  const editor = editorRef.value;

  // 规范化子节点：确保所有顶级内容都在 .para-row 中
  const childNodes = Array.from(editor.childNodes);
  const rows: HTMLElement[] = [];

  for (const node of childNodes) {
    if (node instanceof HTMLElement && node.classList.contains("para-row")) {
      rows.push(node);
    } else if (node instanceof HTMLElement && node.tagName === "DIV") {
      node.classList.add("para-row");
      rows.push(node);
    } else if (node.nodeType === Node.TEXT_NODE && node.textContent?.trim()) {
      const wrapper = document.createElement("div");
      wrapper.className = "para-row";
      wrapper.textContent = node.textContent;
      editor.replaceChild(wrapper, node);
      rows.push(wrapper);
    }
  }

  if (rows.length === 0) {
    const emptyRow = document.createElement("div");
    emptyRow.className = "para-row";
    emptyRow.innerHTML = "<br>";
    const np = newPara("");
    emptyRow.dataset.id = String(np.id);
    editor.appendChild(emptyRow);
    store.paragraphs = [np];
    store.curParaId = np.id;
    updateParaStatus();
    isInternalSync = false;
    return;
  }

  const newParas: Para[] = [];
  for (let i = 0; i < rows.length; i++) {
    const el = rows[i];
    let rawText = el.innerText.replace(/\r?\n$/, "");
    if (rawText === "\n") rawText = "";

    const existingId = Number(el.dataset?.id);
    const existingPara = store.paragraphs.find((p) => p.id === existingId);

    const align = existingPara ? existingPara.align : 0;
    const indentEm = existingPara ? existingPara.indentEm : 0;

    el.style.textAlign = ALIGN_CSS[align];
    el.style.textIndent = indentEm > 0 ? `${indentEm * 13}px` : "0";

    const para: Para = existingPara
      ? { ...existingPara, text: rawText }
      : { id: existingId || Math.floor(Math.random() * 10000000) + 1, text: rawText, align, indentEm };

    el.dataset.id = String(para.id);
    newParas.push(para);
  }

  store.paragraphs = newParas;
  updateCurrentParaFromSelection();
  isInternalSync = false;
}

function updateCurrentParaFromSelection(): void {
  const sel = window.getSelection();
  if (!sel || !sel.anchorNode || !editorRef.value) return;
  let node: Node | null = sel.anchorNode;
  while (node && node !== editorRef.value) {
    if (node instanceof HTMLElement && node.classList.contains("para-row")) {
      const id = Number(node.dataset.id);
      if (id) {
        store.curParaId = id;
        updateParaStatus();
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
      if (node instanceof HTMLElement && node.classList.contains("para-row")) {
        curRow = node;
        break;
      }
      node = node.parentNode;
    }

    const newRow = document.createElement("div");
    newRow.className = "para-row";
    newRow.dataset.id = String(Math.floor(Math.random() * 10000000) + 1);

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

  const lines = text.split(/\r?\n/);
  if (lines.length === 1) {
    document.execCommand("insertText", false, text);
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
    if (node instanceof HTMLElement && node.classList.contains("para-row")) {
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
    newRow.className = "para-row";
    newRow.dataset.id = String(Math.floor(Math.random() * 10000000) + 1);
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
    updateCurrentParaFromSelection();
  }
}

// 监听外部数据变更（如导入 docx / 加载预设）
watch(
  () => store.paragraphs.map((p) => `${p.id}:${p.align}:${p.indentEm}:${p.text}`).join("\u0001"),
  () => {
    syncStoreToDom();
  },
  { immediate: true },
);

// 外部焦点请求（聚焦到某段某偏移）
watch(
  () => pendingFocus.nonce,
  () => {
    void nextTick(() => {
      if (!editorRef.value) return;
      const target = editorRef.value.querySelector<HTMLElement>(`[data-id="${pendingFocus.id}"]`);
      if (!target) return;
      const sel = window.getSelection();
      const rng = document.createRange();
      rng.selectNodeContents(target);
      rng.collapse(true);
      sel?.removeAllRanges();
      sel?.addRange(rng);
      target.scrollIntoView({ block: "nearest" });
    });
  },
);

onMounted(() => {
  document.addEventListener("selectionchange", onSelectionChange);
  syncStoreToDom(true);
});

onBeforeUnmount(() => {
  document.removeEventListener("selectionchange", onSelectionChange);
});
</script>

<template>
  <div
    ref="editorRef"
    class="para-editor"
    :class="{ 'is-empty': isEmpty() }"
    contenteditable="true"
    spellcheck="false"
    @input="onInput()"
    @keydown="onKeydown($event)"
    @paste="onPaste($event)"
    @click.self="focusEmptyArea()"
  ></div>
</template>

