//! 前端 ↔ 后端共享类型：与 src-tauri/src/params.rs 的 UiParams 镜像（camelCase）。

export interface UiParagraph {
  text: string;
  /** 0 左对齐 / 1 居中 / 2 右对齐 */
  align: number;
  /** 首行缩进（字符数 em） */
  indentEm: number;
}

/** 框选文字区域（背景原始像素坐标） */
export interface UiRegion {
  x: number;
  y: number;
  w: number;
  h: number;
  text: string;
  fontPath: string;
  printed: boolean;
  fontSize: number;
  page: number;
}

export interface UiParams {
  fontPath: string;
  backgroundPath: string;
  backgroundPages: string[];
  fontSize: number;
  wordSpacing: number;
  lineSpacing: number;
  wordSpacingSigma: number;
  lineSpacingSigma: number;
  fontSizeSigma: number;
  perturbXSigma: number;
  perturbYSigma: number;
  perturbThetaSigma: number;
  marginTop: number;
  marginBottom: number;
  marginLeft: number;
  marginRight: number;
  fill: string;
  miswriteRate: number;
  miswriteModeIndex: number;
  miswriteStrikeoutStyleIndex: number;
  text: string;
  paragraphs: UiParagraph[];
  regions: UiRegion[];
  endChars: string;
  startChars: string;
  boundsVisible: boolean;
  boundsColor: string;
}

/** render_preview 返回的单页预览 PNG */
export interface PngPage {
  path: string;
  width: number;
  height: number;
}

export interface PresetItem {
  name: string;
  path: string;
}
