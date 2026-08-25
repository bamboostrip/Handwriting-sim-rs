//! Tauri IPC 封装：命令调用 + 原生文件对话框。
//!
//! 命令名/参数与 src-tauri/src/main.rs 一一对应（v2 自动做 camelCase ↔ snake_case）。

import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { PngPage, PresetItem, UiParams } from "./types";

/** 本地文件 → asset 协议 URL（需 tauri.conf.json assetProtocol scope 覆盖） */
export const assetUrl = (path: string): string => convertFileSrc(path);

export const api = {
  renderPreview: (params: UiParams) => invoke<PngPage[]>("render_preview", { params }),
  exportFiles: (params: UiParams, dir: string) =>
    invoke<string[]>("export_files", { params, dir }),
  exportPdf: (params: UiParams, path: string) =>
    invoke<void>("export_pdf", { params, path }),
  /** 返回 [文本, 对齐(0/1/2), 首行缩进像素] 行 */
  importDocx: (path: string, fontSize: number) =>
    invoke<[string, number, number][]>("import_docx", { path, fontSize }),
  importDocument: (path: string) => invoke<string[]>("import_document", { path }),
  listPresets: () => invoke<PresetItem[]>("list_presets"),
  loadPreset: (path: string) => invoke<UiParams>("load_preset", { path }),
  savePreset: (params: UiParams, path: string) =>
    invoke<void>("save_preset", { params, path }),
  defaultPresetDir: () => invoke<string>("default_preset_dir"),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  /** 只读图片文件头，返回 [宽, 高]；失败为 null（框选坐标换算用） */
  imageDimensions: (path: string) =>
    invoke<[number, number] | null>("image_dimensions", { path }),
};

const filters = {
  font: [{ name: "字体文件", extensions: ["ttf", "ttc", "otf"] }],
  image: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }],
  docx: [{ name: "Word 文档", extensions: ["docx"] }],
  document: [{ name: "文档", extensions: ["pdf", "docx"] }],
  preset: [{ name: "预设", extensions: ["json"] }],
  pdf: [{ name: "PDF", extensions: ["pdf"] }],
};

export const dialogs = {
  pickFont: () => open({ filters: filters.font, multiple: false }),
  pickImage: () => open({ filters: filters.image, multiple: false }),
  pickDocx: () => open({ filters: filters.docx, multiple: false }),
  pickDocument: () => open({ filters: filters.document, multiple: false }),
  pickPreset: () => open({ filters: filters.preset, multiple: false }),
  savePresetAs: (defaultDir: string) =>
    save({
      title: "保存预设",
      filters: filters.preset,
      defaultPath: joinPath(defaultDir, "preset.json"),
    }),
  pickFolder: (title?: string) => open({ directory: true, multiple: false, title }),
  savePdf: () =>
    save({ title: "导出 PDF", filters: filters.pdf, defaultPath: "handwrite.pdf" }),
};

function joinPath(dir: string, name: string): string {
  return dir.replace(/[\\/]+$/, "") + "\\" + name;
}
