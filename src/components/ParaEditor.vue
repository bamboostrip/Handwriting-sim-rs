<script setup lang="ts">
//! 逐段文本编辑器：支持跨段多选、批量选中删除、Ctrl+A 全选，
//! 同时保留各段独立对齐（左/中/右）与首行缩进（textIndent）功能，
//! 以及富文本笔迹角色/打印体高亮渲染与选区快速标记。

import { nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import {
  cleanText,
  curParaIndex,
  getHighlightInfo,
  getRoleBadgeInfo,
  isDarkActive,
  newPara,
  pendingFocus,
  registerRoleApplier,
  scheduleRender,
  store,
  updateParaStatus,
} from "../store";

import type { Para } from "../store";
import type { UiTextRun } from "../types";

const ALIGN_CSS = ["left", "center", "right"] as const;

const editorRef = ref<HTMLDivElement | null>(null);
let isInternalSync = false;

// 悬浮快速标记气泡状态
const floatingVisible = ref(false);
const floatingPos = reactive({ x: 0, y: 0 });

function isEmpty(): boolean {
  return (
    store.paragraphs.length === 1 &&
    cleanText(store.paragraphs[0]?.text ?? "").trim() === "" &&
    (!store.paragraphs[0]?.runs || store.paragraphs[0].runs.length === 0)
  );
}

function getRoleTagClass(roleId: number, highlight?: string | null): string {
  if (roleId === 1) return "role-printed";
  if (highlight) {
    const hl = getHighlightInfo(highlight);
    if (hl) return `role-${hl.key.toLowerCase()}`;
  }
  if (roleId === 2) return "role-yellow";
  if (roleId === 3) return "role-green";
  if (roleId === 4) return "role-cyan";
  if (roleId === 5) return "role-magenta";
  return "role-custom";
}

function getRoleTagTitle(roleId: number, highlight?: string | null, fill?: string | null): string {
  const role = store.roles.find((r) => r.id === roleId);
  if (role) return role.name;
  if (roleId === 1) return "打印体";
  const hl = getHighlightInfo(highlight);
  if (hl) return `手写角色 (Word ${hl.name}高亮)`;
  if (fill) return `手写角色 (墨水 ${fill})`;
  return `手写角色 ${roleId - 1}`;
}

/** 递归从 DOM 节点提取 UiTextRun 列表 */
function extractRunsFromNode(
  node: Node,
  parentRoleId = 0,
  parentPrinted = false,
  parentHighlight?: string | null,
  parentFill?: string | null,
  parentFontPath?: string | null,
  parentFontFamily?: string | null,
  parentFontSize?: number | null,
): UiTextRun[] {
  const result: UiTextRun[] = [];

  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent ?? "";
    if (text.length > 0) {
      result.push({
        text,
        style: {
          roleId: parentRoleId,
          printed: parentPrinted || parentRoleId === 1,
          highlight: parentHighlight ?? null,
          fill: parentFill ?? null,
          fontPath: parentFontPath ?? null,
          fontFamily: parentFontFamily ?? null,
          fontSize: parentFontSize ?? null,
        },
      });
    }
    return result;
  }

  if (node.nodeType === Node.ELEMENT_NODE) {
    const el = node as HTMLElement;
    if (el.tagName === "BR") {
      return result;
    }

    let roleId = parentRoleId;
    let printed = parentPrinted;
    let highlight = parentHighlight ?? null;
    let fill = parentFill ?? null;
    let fontPath = parentFontPath ?? null;
    let fontFamily = parentFontFamily ?? null;
    let fontSize = parentFontSize ?? null;

    if (el.dataset.role !== undefined) {
      const parsed = parseInt(el.dataset.role, 10);
      if (!isNaN(parsed)) {
        roleId = parsed;
        if (roleId === 1) printed = true;
      }
    } else if (el.classList.contains("role-printed")) {
      roleId = 1;
      printed = true;
    } else if (el.classList.contains("role-yellow")) {
      roleId = 2;
      highlight = "yellow";
    } else if (el.classList.contains("role-green")) {
      roleId = 3;
      highlight = "green";
    } else if (el.classList.contains("role-cyan")) {
      roleId = 4;
      highlight = "cyan";
    } else if (el.classList.contains("role-magenta")) {
      roleId = 5;
      highlight = "magenta";
    }

    if (el.dataset.highlight) {
      highlight = el.dataset.highlight;
    }
    if (el.dataset.fill) {
      fill = el.dataset.fill;
    }
    // 字体信息经 data 属性往返，避免编辑输入触发重建时丢失
    if (el.dataset.fontPath) {
      fontPath = el.dataset.fontPath;
    }
    if (el.dataset.fontFamily) {
      fontFamily = el.dataset.fontFamily;
    }
    if (el.dataset.fontSize !== undefined && el.dataset.fontSize !== "") {
      const parsed = parseFloat(el.dataset.fontSize);
      if (!isNaN(parsed)) fontSize = parsed;
    }

    for (const child of Array.from(el.childNodes)) {
      result.push(
        ...extractRunsFromNode(child, roleId, printed, highlight, fill, fontPath, fontFamily, fontSize),
      );
    }
  }

  return result;
}

/** 合并相邻且样式相同的 UiTextRun */
function mergeAdjacentRuns(runs: UiTextRun[]): UiTextRun[] {
  const merged: UiTextRun[] = [];
  for (const run of runs) {
    if (!run.text) continue;
    const prev = merged[merged.length - 1];
    const prevRoleId = prev?.style?.roleId ?? 0;
    const curRoleId = run.style?.roleId ?? 0;
    const prevPrinted = Boolean(prev?.style?.printed);
    const curPrinted = Boolean(run.style?.printed);
    const prevHl = prev?.style?.highlight ?? null;
    const curHl = run.style?.highlight ?? null;
    const prevFill = prev?.style?.fill ?? null;
    const curFill = run.style?.fill ?? null;
    const prevFontPath = prev?.style?.fontPath ?? null;
    const curFontPath = run.style?.fontPath ?? null;
    const prevFontFamily = prev?.style?.fontFamily ?? null;
    const curFontFamily = run.style?.fontFamily ?? null;
    const prevFontSize = prev?.style?.fontSize ?? null;
    const curFontSize = run.style?.fontSize ?? null;

    if (
      prev &&
      prevRoleId === curRoleId &&
      prevPrinted === curPrinted &&
      prevHl === curHl &&
      prevFill === curFill &&
      prevFontPath === curFontPath &&
      prevFontFamily === curFontFamily &&
      prevFontSize === curFontSize
    ) {
      prev.text += run.text;
    } else {
      merged.push({
        text: run.text,
        style: {
          roleId: curRoleId,
          printed: curPrinted || curRoleId === 1,
          highlight: curHl,
          fill: curFill,
          fontPath: curFontPath,
          fontFamily: curFontFamily,
          fontSize: curFontSize,
        },
      });
    }
  }
  return merged;
}

/** 将段落内容（含 runs）渲染到单个 para-row DOM 节点 */
function renderRowContent(row: HTMLElement, p: Para): void {
  row.innerHTML = "";
  if (p.runs && p.runs.length > 0) {
    let hasContent = false;
    for (const run of p.runs) {
      if (!run.text) continue;
      hasContent = true;
      const roleId = run.style?.roleId ?? 0;
      const printed = Boolean(run.style?.printed || roleId === 1);
      const effectiveRole = printed ? 1 : roleId;
      const targetRole = store.roles.find((r) => r.id === effectiveRole);

      // Determine highlight & fill
      const highlight =
        run.style?.highlight ??
        targetRole?.highlight ??
        (effectiveRole === 2 ? "yellow" : effectiveRole === 3 ? "green" : effectiveRole === 4 ? "cyan" : null);
      const fill = run.style?.fill ?? targetRole?.fill ?? null;
      // run 级字体覆盖必须写入 data 属性，否则编辑输入重建 runs 时会丢失
      const fontPath = run.style?.fontPath ?? null;
      const fontFamily = run.style?.fontFamily ?? null;
      const fontSize = run.style?.fontSize ?? null;

      if (effectiveRole > 0 || highlight || fill || fontPath || fontFamily || fontSize != null) {
        const span = document.createElement("span");
        const tagClass = getRoleTagClass(effectiveRole, highlight);
        span.className = `run-tag ${tagClass}`;
        span.dataset.role = String(effectiveRole);
        if (highlight) span.dataset.highlight = highlight;
        if (fill) span.dataset.fill = fill;
        if (fontPath) span.dataset.fontPath = fontPath;
        if (fontFamily) span.dataset.fontFamily = fontFamily;
        if (fontSize != null) span.dataset.fontSize = String(fontSize);
        span.title = getRoleTagTitle(effectiveRole, highlight, fill);

        // Apply dynamic color style if custom highlight or role badge
        const hlInfo = getHighlightInfo(highlight);
        const isDark = isDarkActive();
        if (hlInfo) {
          span.style.backgroundColor = isDark ? hlInfo.darkBg : hlInfo.bg;
          span.style.color = isDark ? hlInfo.darkColor : hlInfo.color;
        } else if (targetRole) {
          const badge = getRoleBadgeInfo(targetRole, isDark);
          span.style.backgroundColor = badge.bg;
          span.style.color = badge.color;
        } else if (fill) {
          span.style.color = fill;
        }

        span.textContent = run.text;
        row.appendChild(span);
      } else {
        row.appendChild(document.createTextNode(run.text));
      }
    }
    if (!hasContent) {
      row.innerHTML = "<br>";
    }
  } else {
    if (p.text) {
      row.textContent = p.text;
    } else {
      row.innerHTML = "<br>";
    }
  }
}

/** 将 store.paragraphs 完整同步到编辑器 DOM */
function syncStoreToDom(force = false): void {
  if (!editorRef.value || (isInternalSync && !force)) return;
  const editor = editorRef.value;

  const domRows = Array.from(editor.querySelectorAll<HTMLElement>(":scope > .para-row"));

  const isSame =
    !force &&
    domRows.length === store.paragraphs.length &&
    domRows.every((r, i) => {
      const p = store.paragraphs[i];
      if (!p) return false;
      if (String(p.id) !== r.dataset.id) return false;
      const extracted = extractRunsFromNode(r);
      const merged = mergeAdjacentRuns(extracted);
      const storeRuns =
        p.runs && p.runs.length > 0
          ? p.runs
          : p.text
          ? [{ text: p.text, style: { roleId: 0, printed: false } }]
          : [];
      if (merged.length !== storeRuns.length) return false;
      for (let j = 0; j < merged.length; j++) {
        if (merged[j].text !== storeRuns[j].text) return false;
        const mRole = merged[j].style?.roleId ?? 0;
        const sRole = storeRuns[j].style?.roleId ?? 0;
        const mPr = Boolean(merged[j].style?.printed);
        const sPr = Boolean(storeRuns[j].style?.printed);
        const mHl = merged[j].style?.highlight ?? null;
        const sHl = storeRuns[j].style?.highlight ?? null;
        const mFill = merged[j].style?.fill ?? null;
        const sFill = storeRuns[j].style?.fill ?? null;
        const mFp = merged[j].style?.fontPath ?? null;
        const sFp = storeRuns[j].style?.fontPath ?? null;
        const mFf = merged[j].style?.fontFamily ?? null;
        const sFf = storeRuns[j].style?.fontFamily ?? null;
        const mFs = merged[j].style?.fontSize ?? null;
        const sFs = storeRuns[j].style?.fontSize ?? null;
        if (
          mRole !== sRole ||
          mPr !== sPr ||
          mHl !== sHl ||
          mFill !== sFill ||
          mFp !== sFp ||
          mFf !== sFf ||
          mFs !== sFs
        ) {
          return false;
        }
      }
      return true;
    });

  if (isSame) {
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
    renderRowContent(row, p);
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
    scheduleRender();
    isInternalSync = false;
    return;
  }

  const newParas: Para[] = [];
  for (let i = 0; i < rows.length; i++) {
    const el = rows[i];
    const existingId = Number(el.dataset?.id);
    const existingPara = store.paragraphs.find((p) => p.id === existingId);

    const align = existingPara ? existingPara.align : 0;
    const indentEm = existingPara ? existingPara.indentEm : 0;

    el.style.textAlign = ALIGN_CSS[align];
    el.style.textIndent = indentEm > 0 ? `${indentEm * 13}px` : "0";

    const extracted = extractRunsFromNode(el);
    const mergedRuns = mergeAdjacentRuns(extracted);
    const fullText = mergedRuns.map((r) => r.text).join("");
    const hasRoles = mergedRuns.some(
      (r) =>
        (r.style?.roleId ?? 0) > 0 ||
        r.style?.printed ||
        Boolean(r.style?.highlight) ||
        Boolean(r.style?.fill),
    );

    let rawText = el.innerText.replace(/\r?\n$/, "");
    if (rawText === "\n") rawText = "";

    const finalRawText = fullText.length > 0 ? fullText : rawText;

    const para: Para = {
      id: existingId || Math.floor(Math.random() * 10000000) + 1,
      text: finalRawText,
      align,
      indentEm,
      runs: hasRoles ? mergedRuns : undefined,
    };

    el.dataset.id = String(para.id);
    newParas.push(para);
  }

  store.paragraphs = newParas;
  updateCurrentParaFromSelection();
  scheduleRender();
  isInternalSync = false;
}

function getNodeCharOffset(row: HTMLElement, targetNode: Node, targetOffset: number): number {
  let charCount = 0;

  function traverse(node: Node): boolean {
    if (node === targetNode) {
      if (node.nodeType === Node.TEXT_NODE) {
        charCount += Math.min(targetOffset, (node.textContent || "").length);
      } else if (node.nodeType === Node.ELEMENT_NODE) {
        const children = node.childNodes;
        for (let i = 0; i < targetOffset && i < children.length; i++) {
          charCount += (children[i].textContent || "").length;
        }
      }
      return true;
    }

    if (node.nodeType === Node.TEXT_NODE) {
      charCount += (node.textContent || "").length;
    } else {
      for (const child of Array.from(node.childNodes)) {
        if (traverse(child)) return true;
      }
    }
    return false;
  }

  if (targetNode === row) {
    let count = 0;
    const children = row.childNodes;
    for (let i = 0; i < targetOffset && i < children.length; i++) {
      count += (children[i].textContent || "").length;
    }
    return count;
  }

  traverse(row);
  return charCount;
}

function findNodeAndOffsetAtChar(
  row: HTMLElement,
  targetChar: number,
): { node: Node; offset: number } {
  let currentOffset = 0;
  const walker = document.createTreeWalker(row, NodeFilter.SHOW_TEXT, null);
  let textNode = walker.nextNode();
  let lastNode: Node = row;

  while (textNode) {
    lastNode = textNode;
    const len = (textNode.textContent || "").length;
    if (currentOffset + len >= targetChar) {
      return {
        node: textNode,
        offset: Math.max(0, Math.min(targetChar - currentOffset, len)),
      };
    }
    currentOffset += len;
    textNode = walker.nextNode();
  }

  return {
    node: lastNode,
    offset: (lastNode.textContent || "").length,
  };
}

/** 对当前选区应用指定的笔迹角色 ID（0 为清除标记，1 为打印体，2/3/4 为手写角色） */
function applyRoleToSelection(roleId: number): void {
  if (!editorRef.value) return;
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return;
  const range = sel.getRangeAt(0);

  const editor = editorRef.value;
  if (!editor.contains(range.commonAncestorContainer) && !range.intersectsNode(editor)) {
    return;
  }

  const allRows = Array.from(editor.querySelectorAll<HTMLElement>(":scope > .para-row"));
  if (allRows.length === 0) return;

  let targetRows = allRows.filter((r) => sel.containsNode(r, true) || range.intersectsNode(r));
  if (targetRows.length === 0) {
    let node: Node | null = sel.anchorNode;
    while (node && node !== editor) {
      if (node instanceof HTMLElement && node.classList.contains("para-row")) {
        targetRows = [node];
        break;
      }
      node = node.parentNode;
    }
  }
  if (targetRows.length === 0) {
    const curIdx = Math.max(0, Math.min(curParaIndex(), allRows.length - 1));
    if (allRows[curIdx]) targetRows = [allRows[curIdx]];
  }
  if (targetRows.length === 0) return;

  const isCollapsed = range.collapsed;

  for (let rIdx = 0; rIdx < targetRows.length; rIdx++) {
    const row = targetRows[rIdx];
    const rowTextLen = (row.textContent || "").length;

    let startChar = 0;
    let endChar = rowTextLen;

    if (!isCollapsed) {
      if (row.contains(range.startContainer)) {
        startChar = getNodeCharOffset(row, range.startContainer, range.startOffset);
      } else {
        startChar = 0;
      }

      if (row.contains(range.endContainer)) {
        endChar = getNodeCharOffset(row, range.endContainer, range.endOffset);
      } else {
        endChar = rowTextLen;
      }

      if (startChar > endChar && targetRows.length === 1) {
        const tmp = startChar;
        startChar = endChar;
        endChar = tmp;
      }
    } else {
      let node: Node | null = range.startContainer;
      let insideTag: HTMLElement | null = null;
      while (node && node !== row) {
        if (node instanceof HTMLElement && node.classList.contains("run-tag")) {
          insideTag = node;
          break;
        }
        node = node.parentNode;
      }

      if (insideTag) {
        startChar = getNodeCharOffset(row, insideTag, 0);
        endChar = startChar + (insideTag.textContent || "").length;
      } else {
        startChar = 0;
        endChar = rowTextLen;
      }
    }

    if (startChar === endChar && rowTextLen > 0 && isCollapsed) {
      startChar = 0;
      endChar = rowTextLen;
    }

    const extracted = extractRunsFromNode(row);
    const charRuns: {
      char: string;
      roleId: number;
      printed: boolean;
      highlight: string | null;
      fill: string | null;
    }[] = [];

    if (extracted.length > 0) {
      for (const r of extracted) {
        const rId = r.style?.roleId ?? 0;
        const rPr = Boolean(r.style?.printed || rId === 1);
        const rHl = r.style?.highlight ?? null;
        const rFill = r.style?.fill ?? null;
        for (const ch of r.text) {
          charRuns.push({ char: ch, roleId: rId, printed: rPr, highlight: rHl, fill: rFill });
        }
      }
    } else {
      const txt = row.textContent || "";
      for (const ch of txt) {
        charRuns.push({ char: ch, roleId: 0, printed: false, highlight: null, fill: null });
      }
    }

    const targetRole = store.roles.find((r) => r.id === roleId);
    const targetPrinted = roleId === 1 || Boolean(targetRole?.printed);
    const targetHighlight =
      roleId === 0
        ? null
        : targetRole?.highlight ??
          (roleId === 2 ? "yellow" : roleId === 3 ? "green" : roleId === 4 ? "cyan" : null);
    const targetFill = roleId === 0 ? null : targetRole?.fill ?? null;

    for (let i = startChar; i < endChar && i < charRuns.length; i++) {
      charRuns[i].roleId = roleId;
      charRuns[i].printed = targetPrinted;
      charRuns[i].highlight = targetHighlight;
      charRuns[i].fill = targetFill;
    }

    const newRuns: UiTextRun[] = [];
    for (const cr of charRuns) {
      const last = newRuns[newRuns.length - 1];
      if (
        last &&
        (last.style?.roleId ?? 0) === cr.roleId &&
        Boolean(last.style?.printed) === cr.printed &&
        (last.style?.highlight ?? null) === (cr.highlight ?? null) &&
        (last.style?.fill ?? null) === (cr.fill ?? null)
      ) {
        last.text += cr.char;
      } else {
        newRuns.push({
          text: cr.char,
          style: {
            roleId: cr.roleId,
            printed: cr.printed,
            highlight: cr.highlight ?? null,
            fill: cr.fill ?? null,
          },
        });
      }
    }

    const rowId = Number(row.dataset.id);
    const existingPara = store.paragraphs.find((p) => p.id === rowId);
    const fullText = newRuns.map((r) => r.text).join("");
    const hasRoles = newRuns.some(
      (r) =>
        (r.style?.roleId ?? 0) > 0 ||
        r.style?.printed ||
        Boolean(r.style?.highlight) ||
        Boolean(r.style?.fill),
    );

    const updatedPara: Para = existingPara
      ? { ...existingPara, text: fullText, runs: hasRoles ? newRuns : undefined }
      : {
          id: rowId || Math.floor(Math.random() * 10000000) + 1,
          text: fullText,
          align: 0,
          indentEm: 0,
          runs: hasRoles ? newRuns : undefined,
        };

    const pIdx = store.paragraphs.findIndex((p) => p.id === rowId);
    if (pIdx >= 0) {
      store.paragraphs[pIdx] = updatedPara;
    } else {
      store.paragraphs.push(updatedPara);
    }

    renderRowContent(row, updatedPara);

    if (targetRows.length === 1 && fullText.length > 0) {
      try {
        const startPos = findNodeAndOffsetAtChar(row, startChar);
        const endPos = findNodeAndOffsetAtChar(row, endChar);
        const newRange = document.createRange();
        newRange.setStart(startPos.node, startPos.offset);
        newRange.setEnd(endPos.node, endPos.offset);
        sel.removeAllRanges();
        sel.addRange(newRange);
      } catch {
        // ignore
      }
    }
  }

  if (targetRows.length > 1) {
    try {
      const firstRow = targetRows[0];
      const lastRow = targetRows[targetRows.length - 1];
      const startPos = findNodeAndOffsetAtChar(firstRow, 0);
      const endPos = findNodeAndOffsetAtChar(lastRow, (lastRow.textContent || "").length);
      const newRange = document.createRange();
      newRange.setStart(startPos.node, startPos.offset);
      newRange.setEnd(endPos.node, endPos.offset);
      sel.removeAllRanges();
      sel.addRange(newRange);
    } catch {
      // ignore
    }
  }

  updateCurrentParaFromSelection();
  scheduleRender();
  updateFloatingBubble();
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

function updateFloatingBubble(): void {
  if (!editorRef.value) {
    floatingVisible.value = false;
    return;
  }
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
    floatingVisible.value = false;
    return;
  }
  const range = sel.getRangeAt(0);
  const editor = editorRef.value;
  if (!editor.contains(range.commonAncestorContainer) && !range.intersectsNode(editor)) {
    floatingVisible.value = false;
    return;
  }

  const text = sel.toString().trim();
  if (!text) {
    floatingVisible.value = false;
    return;
  }

  const rect = range.getBoundingClientRect();
  const editorRect = editor.getBoundingClientRect();

  if (rect.width === 0 && rect.height === 0) {
    floatingVisible.value = false;
    return;
  }

  const centerX = rect.left + rect.width / 2 - editorRect.left;
  const clampedX = Math.max(160, Math.min(centerX, editorRect.width - 160));

  let topY = rect.top - editorRect.top - 6;
  if (topY < 32) {
    topY = rect.bottom - editorRect.top + 34;
  }

  floatingPos.x = clampedX;
  floatingPos.y = topY;
  floatingVisible.value = true;
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

function onEditorClick(e: MouseEvent): void {
  const sel = window.getSelection();
  if (sel && !sel.isCollapsed) return;

  if (e.target === editorRef.value && editorRef.value) {
    const lastRow = editorRef.value.querySelector<HTMLElement>(":scope > .para-row:last-child");
    if (lastRow) {
      const rng = document.createRange();
      rng.selectNodeContents(lastRow);
      rng.collapse(false);
      sel?.removeAllRanges();
      sel?.addRange(rng);
    }
  }
}

// 监听外部数据变更（如导入 docx / 加载预设）
watch(
  () =>
    store.paragraphs
      .map(
        (p) =>
          `${p.id}:${p.align}:${p.indentEm}:${p.text}:${p.runs ? JSON.stringify(p.runs) : ""}`,
      )
      .join("\u0001"),
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

function onSelectionChange(): void {
  if (
    document.activeElement === editorRef.value ||
    editorRef.value?.contains(document.activeElement) ||
    (window.getSelection()?.rangeCount &&
      editorRef.value?.contains(window.getSelection()!.getRangeAt(0).commonAncestorContainer))
  ) {
    updateCurrentParaFromSelection();
    updateFloatingBubble();
  } else {
    floatingVisible.value = false;
  }
}

onMounted(() => {
  document.addEventListener("selectionchange", onSelectionChange);
  syncStoreToDom(true);
  const unregister = registerRoleApplier(applyRoleToSelection);
  onBeforeUnmount(() => {
    unregister();
    document.removeEventListener("selectionchange", onSelectionChange);
  });
});
</script>

<template>
  <div class="para-editor-wrapper">
    <!-- 悬浮快速标记气泡 (选中文本时出现) -->
    <div
      v-if="floatingVisible"
      class="role-bubble"
      :style="{ left: `${floatingPos.x}px`, top: `${floatingPos.y}px` }"
      @mousedown.prevent
    >
      <button
        v-for="role in store.roles.filter((r) => r.id > 0)"
        :key="role.id"
        type="button"
        class="bubble-btn"
        :title="role.name"
        @mousedown.prevent
        @click="applyRoleToSelection(role.id)"
      >
        <span
          class="bubble-color-dot"
          :style="{
            backgroundColor: getRoleBadgeInfo(role, isDarkActive()).bg,
            color: getRoleBadgeInfo(role, isDarkActive()).color,
            borderColor: getRoleBadgeInfo(role, isDarkActive()).color,
          }"
        >
          {{ getRoleBadgeInfo(role, isDarkActive()).icon }}
        </span>
        {{ role.name }}
      </button>
      <button
        type="button"
        class="bubble-btn btn-clear"
        title="清除标记 (恢复默认手写)"
        @mousedown.prevent
        @click="applyRoleToSelection(0)"
      >
        ✕ 清除
      </button>
    </div>

    <div
      ref="editorRef"
      class="para-editor"
      :class="{ 'is-empty': isEmpty() }"
      contenteditable="true"
      spellcheck="false"
      @input="onInput()"
      @keydown="onKeydown($event)"
      @paste="onPaste($event)"
      @click="onEditorClick($event)"
    ></div>
  </div>
</template>
