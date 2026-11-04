//! 前端 ↔ 后端共享类型：与 src-tauri/src/params.rs 的 UiParams 镜像（camelCase）。

export interface UiTextRunStyle {
  roleId?: number;
  highlight?: string | null;
  fontPath?: string | null;
  fontFamily?: string | null;
  fontSize?: number | null;
  fill?: string | null;
  printed?: boolean;
}

export interface UiTextRun {
  text: string;
  style?: UiTextRunStyle;
}

export interface DocxImportOutput {
  paragraphs: UiParagraph[];
  docFontFamily?: string | null;
}

export interface DocumentImportResult {
  pages: string[];
  regions: UiRegion[];
}

export interface UiParagraph {
  text: string;
  /** 0 左对齐 / 1 居中 / 2 右对齐 */
  align: number;
  /** 首行缩进（字符数 em） */
  indentEm: number;
  runs?: UiTextRun[];
}

export interface UiHandwritingRole {
  id: number;
  name: string;
  highlight?: string | null;
  fontPath: string;
  printed: boolean;
  fontSize?: number | null;
  fill?: string | null;
  wordSpacing?: number | null;
  lineSpacing?: number | null;
  fontSizeSigma?: number | null;
  wordSpacingSigma?: number | null;
  lineSpacingSigma?: number | null;
  perturbXSigma?: number | null;
  perturbYSigma?: number | null;
  perturbThetaSigma?: number | null;
  miswriteRate?: number | null;
  miswriteStrikeoutStyleIndex?: number | null;
}

/** 框选文字区域（背景原始像素坐标） */
export interface UiRegion {
  x: number;
  y: number;
  w: number;
  h: number;
  text: string;
  /** 绑定的角色 ID（0 为默认角色） */
  roleId?: number;
  /** 绑定的高亮颜色名称（如 "yellow", "cyan", "pink", null = 无高亮） */
  highlight?: string | null;
  fontPath: string;
  printed: boolean;
  fontSize: number;
  page: number;
  /** 所属表格序号（1 基；null = 非表格区域） */
  tableIndex?: number | null;
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

/** 图形标注类型 id（与 src-tauri/src/params.rs、core shapes.rs 的 as_id 逐字一致） */
export type UiShapeKind =
  | "line" | "arrow" | "polyline" | "rect" | "ellipse" | "triangle"
  | "brace_left" | "brace_right" | "bracket_left" | "bracket_right"
  | "check" | "underline";

export interface UiShape {
  kind: UiShapeKind;
  /** 1 基页码 */
  page: number;
  /** 背景原始像素包围盒（允许负宽高，后端归一化） */
  x: number; y: number; w: number; h: number;
  /** 显式顶点（line/arrow 必须给 2 点，用于保留方向），背景原始像素 */
  points?: [number, number][];
  /** 线宽（原始像素） */
  thickness: number;
  /** 手绘抖动幅度（原始像素，0 = 直尺） */
  jitter: number;
  /** 实心（rect/ellipse/triangle 生效） */
  filled: boolean;
  /** "#RRGGBB"；null/缺省 = 跟随主墨色 */
  fill?: string | null;
  /** 每图形随机种子 */
  seed: number;
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
  /** 图形标注（背景原始像素坐标，与 regions 同一坐标系） */
  shapes: UiShape[];
  roles?: UiHandwritingRole[];
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

export interface SystemFontItem {
  name: string;
  family: string;
  path: string;
}

