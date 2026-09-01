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
  /** 对齐方式：0 左 / 1 居中 / 2 右（应用于区域整体文本） */
  align: number;
  /** 首行缩进（字符数 em；0 = 无） */
  indentEm: number;
  /** 区域内各段落排版信息（各段独立设置对齐与缩进） */
  paragraphs?: UiParagraph[];

  // ---- 逐区域覆盖项（null = 跟随主设置）----
  wordSpacing?: number | null;
  lineSpacing?: number | null;
  fontSizeSigma?: number | null;
  wordSpacingSigma?: number | null;
  lineSpacingSigma?: number | null;
  perturbXSigma?: number | null;
  perturbYSigma?: number | null;
  perturbThetaSigma?: number | null;
  /** 错字率 0~1 */
  miswriteRate?: number | null;
  /** 涂改方式索引；null = 跟随主设置 */
  miswriteStrikeoutStyleIndex?: number | null;
  /** 文字颜色 #RRGGBB；null = 跟随主设置 */
  fill?: string | null;
  /** 区域边距（像素；null / 0 = 紧贴框边界，默认 0） */
  marginTop?: number | null;
  marginBottom?: number | null;
  marginLeft?: number | null;
  marginRight?: number | null;
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

export interface UpdateInfo {
  version: string;
  tagName: string;
  title: string;
  body: string;
  htmlUrl: string;
  assetName: string;
  assetUrl: string;
  assetSize: number;
  hasUpdate: boolean;
}

export interface DownloadProgressPayload {
  received: number;
  total: number;
  percent: number;
}

