# issue #6 前端 / 命令层可行性研究：图形标注图层（Shape Layer）

> 对应 issue：[#6【建议】增加符号图形](https://github.com/bamboostrip/Handwriting-sim-rs/issues/6)
> 分支：`feat/symbol-shapes`（未切分支、未提交）
> 调研范围：前端（`src/`）+ Tauri 命令层（`src-tauri/src/params.rs`、`main.rs`）+ 必要的引擎接口只读核对。
> 配套文档：`docs/05-symbol-shapes.md`（方案与引擎侧设计，第 221-224 行明确把逐文件前端清单指向本文）。
> 调研方式：**只读代码，未编译、未运行、未改动任何现有文件**；本文只新建本文件。
> 行号基准：2026-11-04 07:44 工作区快照。注意 `crates/core/src/core/shapes.rs` 是同事**刚刚落盘、尚未提交**的 POC（`git status` 显示 `?? crates/core/src/core/shapes.rs`，`layout.rs`/`mod.rs` 已改），其行号随后续编辑可能漂移。

---

## 0. 结论速览

| 问题 | 结论 | 依据 |
| --- | --- | --- |
| 需求能否实现 | **能**。图形层与现有「矢量基元 → 手绘掩码 → 扰动上色」管线天然契合，无需新渲染后端、无需新 IPC 命令 | `engine.rs:686-824`（唯一合成点）、`shapes.rs:164-302`（POC 已能出图）、`params.rs:407-632`（整包 `UiParams` 透传） |
| 有没有硬阻塞 | **没有技术硬阻塞**。但有 **3 个必须先拍板的产品/契约决策**：①`ShapeKind` 的 JSON 形态；②持久化范围（预设不存图形 → 重启即丢）；③折线（Polyline）是否进 MVP | 见 §2.3、§2.4、§7 |
| 最大风险 | **「预览 = 导出」的几何一致性**（`scaled_params_for` 漏缩放）与**契约不一致**（POC 的 `ShapeSpec` 与设计稿字段有 5 处差异） | `engine.rs:71-168`、`shapes.rs:50-101` vs `05-symbol-shapes.md:99-124` |
| 主要工作量落在哪 | 前端交互（约 40%）> 引擎图元（约 30%）> 面板/属性（20%）> 命令层（10%） | §6.2 |
| 6 类图形成本最高 | **大括号**（几何最贵）> **折线**（前端交互最贵）> **椭圆**（观感调参贵，非代码量）；`▶` 几何最便宜但「当项目符号用」最不划算 | §7.3 |
| 工期（含引擎） | POC 已完成（core 侧）；**可用 MVP 7~9 人日**；**完整版 11~14 人日** | §6.2、与 `05:238-246` 分项核对 |

---

## 1. 现状梳理

### 1.1 端到端调用链（图形层只需接在最后一跳）

```
[Vue] store.buildParams()                         src/store.ts:942
   └─ api.renderPreview / exportFiles / exportPdf / savePreset
                                                  src/api.ts:23,24,26,35
        └─ Tauri command                          src-tauri/src/main.rs:110 / 165 / 182 / 267
             └─ checked_params()                   main.rs:54-61
                  ├─ UiParams::to_handwriting_params()   params.rs:407-632
                  └─ HandwritingParams::validate_with()  crates/core/src/core/models.rs:583-...
                       └─ engine::{render_all_pages_preview, export, export_pdf}
                                                  main.rs:119 / 172 / 185
                            └─ DefaultEngine::{preview_pages_impl, render_pages}   engine.rs:933 / 837
                                 └─ generate_pages_with()   engine.rs:686-824  ← 唯一图形合成点
```

关键结论：**预览、PNG 导出、PDF 导出共用 `generate_pages_with`**，因此图形只要在该函数的每页合成循环里画一次，三条出口全部自动继承（§3.5）。

### 1.2 `src/components/PreviewPane.vue`（交互骨架，717 行）

| 能力 | 位置 | 现状要点 | 图形层复用方式 |
| --- | --- | --- | --- |
| 坐标链 | `PreviewPane.vue:20-21`（注释）、`:98-101` | 区域存**背景原始像素**；`factor = draw.w / store.bgNatW`，显示像素 = 原始像素 × factor | 图形用同一套：存原始像素，DOM 覆盖层乘 `factor` |
| 局部坐标 | `:279-282 localPoint()` | 相对 `interactEl`（图纸）的 CSS 像素 | 直接复用 |
| 缩放锚点 | `:294-302 boxPoint()` | 相对容器，需扣除 1px 边框 | 直接复用 |
| 缩放 | `:304-339 zoomTo/zoomIn/zoomOut/setFit/setActual`；`:341-351 onWheel` | 以光标为中心指数步进；拖拽/调整中禁用 | 需把「图形拖拽中」加入禁用条件（`:345`） |
| 平移 | `:353-363 beginPan()`；`:492-510 onMove` | 中键或空白左键，3px 阈值 | 需把「图形拖拽中」加入禁用条件（`:356`） |
| 框选新建 | `:365-377 toSrcRect/finishRect`；`:431-442` 起框；`:518-523` 拉伸；`:554-560` 落框 | 显示矩形 → `/factor` → `clampRect`（整数、最小 8） | 图形创建**不能直接复用 `clampRect`**：它四舍五入取整（`store.ts:2115-2135`），图形建议保留 f32 |
| 命中已有区域 | `:244-260 findRegionAt()` | **包围盒内含点即命中**，从后往前（后画的在上） | 图形不能照抄：空心大框会挡住底下文字区域，建议「描边距离命中」 |
| 调整态判定 | `:192-207 hasEdit/ebBase/editBox/cur` | 只认 `store.editingIndex` | 需并列 `editingShapeIndex`，二者互斥 |
| 8 向手柄 | `:209-230 zoneAt()`（8px 容差） | 角/边/移动，纯函数式但读 `cur()` | 抽出 `zoneAtRect(rect,px,py)` 复用；线/箭头另用端点手柄 |
| 调整写回 | `:382-400 beginAdjust`；`:456-490 applyAdjust`；`:536-552` 松手才写回 | 最小显示尺寸 8px、钳制在图纸内、**几何没变不写回** | 图形照抄这套「草稿在组件内、松手才 commit」的策略，避免拖动触发 300ms 防抖渲染风暴 |
| 光标 | `:262-276 cursorStyle()` | panning > adjusting > 手柄 > 框选十字 > 区域悬浮 > 抓手 | 需插入图形模式与端点手柄分支 |
| 双击 | `:570-578 onDblClick()` | 命中最上层区域 → 打开属性对话框 | 图形双击 → 定位到属性面板（图形无需文本对话框） |
| 点空白退出 | `:580-585 onBoxDown()` | 调 `cancelRegionEdit()` | 需同时取消图形选中 |
| 模板/覆盖层 | `:624-655` | `.draw-area` 内依次：底图 → 悬浮高亮 → 橡皮筋 → 编辑框+手柄 → `.interact-layer` | 新增两层：**选中轮廓 SVG**（`pointer-events:none`）与图形草稿层，位置在 `.interact-layer` 之前 |
| Esc | `src/App.vue:124-135` | 仅当 `editingIndex/selectedRegionIndex >= 0` 时 `cancelRegionEdit()` | 条件与动作都要扩展（§3.2.4） |
| 聚焦 watcher | `:137-156` | 用 `natW/bgNatW` 换算后 `focusRectView`（`:144-145`） | 图形「🔍」需并列一套 `shapeFocusToken`，不要复用 `previewFocusIndex` |

**现状里两个容易踩的细节：**

1. `findRegionAt` 是**包围盒命中**（`:255`），而图形多为空心；若图形也用包围盒命中，一个大椭圆会挡住它内部的所有文字区域点击。建议图形用「描边距离 ≤ 8 显示像素」判定（与 `zoneAt` 的 8px 容差一致，`:213`）。
2. 预览是**位图**（`main.rs:138-150` 落盘 PNG，前端 `<img>` 显示 `PreviewPane.vue:625`），图形一旦提交就烧进位图；拖动过程中只能移动 DOM 轮廓，位图里的旧图形要等下一次渲染（防抖 300ms，`store.ts:1047`）才更新 → 会出现「旧墨迹 + 新虚线框」的短暂重影。MVP 可接受，需在交互文案/视觉上说明（§3.2.4）。

### 1.3 `src/store.ts`（状态与流程，2553 行）

| 事项 | 位置 | 说明 |
| --- | --- | --- |
| 响应式 store | `:389-494` | 全部表单状态集中在一个 `reactive` 对象 |
| 区域状态 | `:428-434` | `regionMode/regions/editingIndex/highlightIndex/selectedRegionIndex` |
| 区域对话框状态 | `:436-440` | `pendingRect/dialogOpen/dialogTargetIndex/dialogDraft` |
| 预览状态 | `:445-455` | `previewPages/pageIndex/bgNatW/bgNatH/rendering` |
| 参数构建 | `:942-996` | `regions: store.regions.map(r => ({...r}))`（`:989`）；**图形在此处加一行** |
| 渲染 | `:1014-1045 doRender`；`:1047-1049 scheduleRender`（`useDebounceFn(doRender,300)`） | 带 `renderSeq` 代次守卫 |
| 自动重渲染 | `src/App.vue:102-106` | `JSON.stringify(buildParams())` 变化即 `scheduleRender()`；`dialogOpen` 时暂停 |
| 框选模式 | `:2137-2141 setRegionMode()` | 进入框选时清 `editingIndex` —— 图形模式需与它互斥 |
| 新建/编辑对话框 | `:2171-2176`、`:2179-2202`、`:2204-2208`、`:2211-2241` | 草稿式；图形不需要对话框，可直接落库 |
| 几何写回 | `:2334-2339 updateRegionGeometry()` | 写回后 `scheduleRender()` |
| 选中/定位/删除/清空 | `:2341-2385` | `jumpToRegion/focusRegion/deleteSelectedRegion/clearRegions` |
| 标签 | `:2095-2112 regionLabel()` | 图形列表建议做同构的 `shapeLabel()` |
| 预设应用 | `:1581-1653` | **`:1639` 只清空 `regions`** —— 换纸张后图形坐标同样失效，必须一并清空 |
| 预设脏标记 | `:1658-1689` | 快照字段白名单；`regions` 不在其中（文档内容不算版式改动），图形应保持一致 |
| 预设载入/保存 | `:1790-1816 selectPreset`；`:1824-1841 savePresetToDialog` | 都走 `buildParams()` / `applyPreset()` |
| 导出 | `:1845-1864 exportFiles`；`:1866-1888 exportPdf` | `buildParams()` 整包下发 |
| 内容判定 | `:626-632 hasContent`；`:637-640 workflowMode` | **未计图形**：纯图形页面会被判成 empty，`PreviewPane.vue:689` 会用 `<QuickStart>` 盖住预览 |

**区域导入/清理点（加图形后必须同步清空）：** `store.ts:1479`（docx 导入）、`:1515`（文档导入）、`:1639`（应用预设）、`:2378`（清空区域）、`:898`（引导示例）。这些位置都是「换背景/换内容 → 旧坐标失效」，图形若不跟着清，会残留到新纸上。

### 1.4 `src/types.ts`（前端契约，169 行）

- `UiRegion`：`:58-101`，坐标字段是 `number`（后端 `i32`），`fill?: string | null`（`:95`）、`page: number`（`:71`）、`margin*`（`:96-100`）。
- `UiParams`：`:103-132`，`regions: UiRegion[]`（`:126`）、`roles?`（`:127`）。前端只有 `store.ts:942 buildParams()` 一处构造 `UiParams`（grep 全仓确认），因此加必填字段只需改这一处。
- 该文件头注释明确「与 `src-tauri/src/params.rs` 的 `UiParams` 镜像（camelCase）」（`:1`）——新增 `UiShape` 必须两边同时改。

### 1.5 `src-tauri/src/params.rs`（命令层映射，931 行）

| 事项 | 位置 | 说明 |
| --- | --- | --- |
| `UiRegion` | `:178-235` | `i32` 坐标、`fill: Option<String>`（`:226`）、`page: i32`（`:201`） |
| `UiRegion ← TextRegion` | `:241-300` | 反向只用于「文档导入 → 前端」 |
| `UiParams` | `:303-342` | **结构级** `#[serde(rename_all="camelCase", default)]`（`:304`），`regions`（`:334`）、`roles`（`:335-336`） |
| `UiParams::default()` | `:344-378` | **手写 `impl Default`**，逐字段列出（`:370-371` 为 regions/roles）→ 加 `shapes` 必须在这里补一行，否则编译不过 |
| `to_handwriting_params()` | `:407-632` | 区域映射在 `:503-602`；`HandwritingParams{...}` 构造在 `:604-631`。图形映射加在同层 |
| `from_handwriting_params()` | `:636-675` | `text/paragraphs/regions` 明确置空（`:666-668`）；新增 `shapes` 同理（预设不含图形） |
| 校验在哪 | **不在 params.rs** | `main.rs:54-61 checked_params()` → `models.rs:583 validate_with()`；`params.rs:405-406` 注释也写明「只做映射，不做校验」。图形校验应放 core（§3.2.8），命令层只负责颜色解析错误（参考区域 `:507-513`） |

`src/api.ts` 全部是 `invoke` 薄封装（`:22-63`），命令参数都是整包 `UiParams`：**图形不需要新增命令，也不需要改 `generate_handler!`（`main.rs:404-412`）**，这是命令层成本极低的原因。

### 1.6 core 侧现状（只读核对，不由本文实现）

**已落盘的 POC（未提交）** `crates/core/src/core/shapes.rs`：

| 内容 | 位置 |
| --- | --- |
| 坐标约定：页面像素；`offset_x/offset_y` 用于局部坐标平移 | `shapes.rs:14-16`、`:164-168` |
| `BraceSide`/`BracketSide` | `:28-45` |
| `ShapeKind`（`rename_all="snake_case"`；`Brace(side)`/`Bracket(side)` 是**元组变体**） | `:47-72` |
| `ShapeSpec` 字段 | `:74-101` |
| 默认值（thickness 2.4px、jitter 1.4px、seed 1、黑色） | `:111-125` |
| `rasterize_shape(spec, mask, page_w, page_h, offset_x, offset_y)` | `:170-290` |
| `rasterize_shapes(...)` | `:293-302` |
| 负宽高归一化 `normalized_bbox` | `:305-312` |
| 线/箭头端点：优先 `points` 前两点，否则包围盒对角线 | `:315-327` |
| 大括号实现 | `:576-610` |
| 椭圆 8 段贝塞尔 + 起止过冲 | `:509-575` |
| 单元测试（12 种图元都出墨、同 seed 可复现、填充覆盖内部、零抖动不出包围盒） | `:655-775` |

**引擎侧接线点（同事负责）：**

- `HandwritingParams` 定义 `crates/core/src/core/models.rs:485-538`；`regions` 用字段级 `#[serde(default)]`（`:497-498`）——`shapes` 必须同样加字段级 default。
- `validate_with()` `models.rs:583-...`：区域校验在 `:654-672`；图形校验照此追加。
- `scaled_params_for()` `engine.rs:71-168`：区域缩放循环在 `:125-166`；**图形缩放必须加在这里**。
- `generate_pages_with()` `engine.rs:686-824`：区域条目在 `:711-774`，页数计算在 `:776-781`，**每页合成循环在 `:785-822`**（区域先合成 `:789-805`，主文字后合成 `:806-820`）。
- `region_local_params()` `engine.rs:304-...`：会 `clone` 整个 params 并清空 `regions`（`:308`），**也应清空 `shapes`**，否则每个区域都白拷贝一份图形数组。
- 预览降采样：`PREVIEW_MAX_WIDTH=4096`（`engine.rs:46`），`load_background_for_preview`（`:211-276`）调用 `scaled_params_for`（`:225`、`:235`）。
- 导出：`export()`（`:983-985`）→ `save_all`（`:863-909`）→ `render_pages`（`:837-861`）；`export_pdf()`（`:999-1004`）内部同样 `render_pages`（`:1004`，PDF 是 300 DPI 整页位图，`main.rs:179-180` 注释）。

**POC 与设计稿的 5 处差异（契约冻结前必须解决，见 §2.3）：**

| 项 | 设计稿 `05-symbol-shapes.md` | POC `shapes.rs` | 影响 |
| --- | --- | --- | --- |
| `kind` 枚举 | `BraceLeft/…` 扁平 15 种（`:132`） | `Brace(BraceSide)` 元组 → JSON 为 `{"brace":"left"}`（`:47-72`） | 前端 TS 联合类型无法扁平，必须改 |
| `jitter` 单位 | 0~1 归一化（`:117-118`） | **像素幅度**，默认 1.4（`:94-95`） | 属性面板滑杆范围要按 px，不是 0~1 |
| `points` 坐标 | 归一化 0~1（`:107-108`） | **页面绝对像素**（`:87-89`、`:195`） | 降采样缩放必须连 points 一起乘 scale |
| `fill` | `Option<[u8;3]>`（可跟随主墨色，`:112`） | `[u8;3]` 必填（`:92-93`） | 「跟随主墨色」需在命令层解析后写死 RGB |
| 页码/层级/角色 | `page/z/role_id`（`:104,114,121-122`） | **都没有** | 多页文档无法把图形放到第 2 页，必须补 |

**POC 样张（工作区实测产物）**：`crates/core/examples/shapes_poc.rs`（未提交）产出 `tmp_shapes/shapes_poc.png`（1200×800，07:46 生成），同一张已归档到 `docs/screenshots/issue6-shapes-poc.png`（`tmp_shapes/` 已被 `.gitignore` 忽略）。样张里绿框框字、红圈圈数字 3、`[ ]`、`{ }`、竖线分隔、箭头/折线流程、`▶` 列表符号、`✓` 勾都在，与 issue 截图的 6 类用法一一对应，可直接作为风格验收样张。注意：该示例是**直接调用 `rasterize_shape` 画进掩码**，`engine.rs` 目前尚无改动（`git status` 未列 engine.rs），所以「预览/导出自动继承」仍属待接线工作（§3.5）。

---

## 2. 数据模型与持久化

### 2.1 建议的模型（前端 / 命令层视角）

**原则：坐标系与 `UiRegion` 完全一致 —— 背景原始像素**（`types.ts:57`、`params.rs:181`）。这样预览覆盖层与 `PreviewPane` 现有 `factor` 换算（`:99-101`）可以原样复用；引擎侧用 `scaled_params_for` 统一降采样。

```ts
// src/types.ts 新增
export type ShapeKind =
  | "line" | "arrow" | "polyline" | "rect" | "ellipse" | "triangle"
  | "brace_left" | "brace_right" | "bracket_left" | "bracket_right"
  | "check" | "underline";

export interface UiShape {
  kind: ShapeKind;
  /** 背景原始像素包围盒（允许负宽高，后端归一化） */
  x: number; y: number; w: number; h: number;
  /** 显式顶点（line/arrow/polyline 必填 2+ 个），背景原始像素 */
  points?: [number, number][];
  /** 线宽（原始像素） */
  thickness: number;
  /** 手绘抖动幅度（原始像素，0 = 直尺） */
  jitter: number;
  /** 是否填充（rect/ellipse/triangle 生效） */
  filled: boolean;
  /** 墨色 #RRGGBB；null = 跟随主墨色（命令层解析成 RGB 后下发） */
  fill?: string | null;
  /** 1 基页码 */
  page: number;
  /** 抖动随机种子（决定同一图形的抖动形态） */
  seed: number;
}

// UiParams 增加（types.ts:126 regions 旁）
shapes: UiShape[];
```

```rust
// src-tauri/src/params.rs 新增镜像（与 UiRegion 同风格：camelCase + default）
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiShape {
    pub kind: ShapeKind,      // 或 UiShapeKind 字符串枚举；见 §2.3
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
    pub points: Vec<[f32; 2]>,
    pub thickness: f32,
    pub jitter: f32,
    pub filled: bool,
    pub fill: Option<String>,
    pub page: i32,
    pub seed: u64,
}
```

### 2.2 放哪：`HandwritingParams.shapes` + `UiParams.shapes`

- `HandwritingParams`（`models.rs:485-538`）是「一次渲染的完整参数」，与 `regions` 同级放 `shapes` 语义正确：图形是**文档内容**（跟着页面走），不是版式参数。
- `UiParams`（`params.rs:303-342`）是它的前端镜像，加 `pub shapes: Vec<UiShape>`，并在 `to_handwriting_params()`（`:604-631` 的构造处）与 `from_handwriting_params()`（`:636-675`）各加一条映射。
- **`UiParams::default()` 是手写的**（`:344-378`），必须补 `shapes: Vec::new()`；`UiParams` 还 `derive(PartialEq)`（`:303`），所以 `UiShape` 也要 `PartialEq`。
- `UiParams` 的结构级 `#[serde(default)]`（`:304`）意味着**旧前端 / 旧调用方不带 `shapes` 也能反序列化**；`HandwritingParams` 没有结构级 default，所以 `shapes` 字段必须写字段级 `#[serde(default)]`（对齐 `regions` 的 `:497-498`）。

### 2.3 `#[serde(default)]` 到底保护了什么（纠正一个常见误解）

任务里写「`#[serde(default)]` 保证旧预设 JSON v2 兼容」，严格说**预设兼容不靠 serde**：

1. 预设载入是 `presets::load()`（`presets.rs:193-202`）→ `from_preset_map()`（`:118-182`）→ **白名单逐字段读取**（`:126-180`），未知字段天然忽略；随后 `UiParams::from_handwriting_params(&hp)`（`main.rs:260-263`）在 Rust 内构造，**不经过 serde**。
2. 预设保存是 `save_preset`（`main.rs:267-272`）→ `presets::save()` → `to_preset_map()`（`presets.rs:86-115`），**白名单逐字段写出**，`regions`/`shapes` 根本不会进 JSON。
3. `#[serde(default)]` 真正保护的是：**前端 `invoke` 传参**（旧前端不带字段）与**外部 JSON 直接反序列化 `HandwritingParams`**（测试/CLI）。`HandwritingParams` 字段级 default 是必须的，但与「预设兼容」是两件事。

**结论：** 只要照 `regions` 的做法（字段级 default + 白名单不写），旧预设 JSON v2 100% 照常载入；**新预设也不会包含图形**（见 §2.4）。

### 2.4 预设 JSON v2 与 Python 版互通风险

**现状事实：**

- 预设格式是 `{"version":2,"params":{...}}`（`presets.rs:1-7`、`:185-190`），README 明确承诺「**与 Python 版完全互通**（只保存排版参数，不含文本内容）」（`README.md:192`）。
- `to_preset_map` 的白名单**不含 `regions`**（`presets.rs:86-115`），也就是说**今天的文字区域本来就不随预设保存**；「当前方案」只保存版式（`ParamsPanel.vue:137-157`）。
- 图形与 `regions` 同级 → 默认同样**不进预设**。`applyPreset` 又会清空区域坐标（`store.ts:1639`），图形必须跟着清（§1.3）。

**两条路线与风险：**

| 路线 | 做法 | 风险 |
| --- | --- | --- |
| A（推荐，v1） | 图形**不进预设**，与 `regions` 一致 | 用户切预设/重启后图形丢失。需 UI 明示「预设只存版式，不含文字区域与图形」；或另立「工程文件」需求。对 Python 互通 **零影响** |
| B（未来） | 扩展 `to_preset_map`/`from_preset_map` 写入 `shapes` | ①Rust 侧读旧预设没问题（白名单 + default）；②**Python 版载入**：若其 loader 也是白名单（与 Rust 同源），未知键会被忽略 → 能打开但图形消失；若严格校验则直接报错（**本机没有 Python 版仓库，无法验证，需确认**）；③**Python 另存会丢图形**；④违反 README:192「完全互通」承诺，等于把预设格式升到 v3，需要 Python 同步 |

**建议：** v1 走 A。若产品要求「图形随方案保存」，正确做法是新增**工程文件**（`*.hwproj.json`，序列化整个 `UiParams`：text/paragraphs/regions/shapes），而不是污染「预设=版式模板」的语义。这也顺带解决 §2.5 的历史缺口。

### 2.5 当前根本没有「工程持久化」（图形会继承同一缺口）

`grep` 全前端：`localStorage` 只存主题 / 面板折叠 / 更新检查标记（`store.ts:29-101`、`:507`、`:2419-2426`），**没有任何函数把 `regions` 或正文写回磁盘**；唯一的保存入口 `savePresetToDialog`（`store.ts:1824-1841`）走 `buildParams()` → `save_preset` → 白名单（不含 text/regions/shapes）。也就是说：

- 文字区域目前就是「会话内有效，导出即产物」，关掉软件即丢；
- 图形沿同一模式设计**不算新增缺陷**，但 issue 用户很可能会预期「下次还能改」，需在验收时明确。若要求持久化 → 参考 §2.4 的工程文件方案（另计 1.5~2 人日）。

---

## 3. 最小改造清单

### 3.1 文件级总表

| 文件 | 类型 | 主要内容 | 预估净增 LOC | 依赖 |
| --- | --- | --- | --- | --- |
| `crates/core/src/core/shapes.rs` | 已有 POC（同事） | 图元几何 + 手绘抖动 + 单测 | 775（已存在，未提交） | — |
| `crates/core/src/core/models.rs` | 改（同事） | `Shape` 包装 / `page` 字段、`validate_with` 图形校验 | +40~60 | 契约冻结 |
| `crates/core/src/core/engine.rs` | 改（同事） | `scaled_params_for` 缩放、`generate_pages_with` 合成、`region_local_params` 清 shapes、页数计算 | +60~80 | models |
| `src-tauri/src/params.rs` | 改 | `UiShape` + `From` + 两处映射 + `Default` + 颜色解析 + 单测 | +130~160 | models 契约 |
| `src-tauri/src/main.rs` | **不改** | 无新命令；`checked_params` 自动透传 | 0 | — |
| `crates/core/src/core/presets.rs` | **不改**（路线 A） | 白名单不含 shapes | 0 | — |
| `src/types.ts` | 改 | `ShapeKind` + `UiShape` + `UiParams.shapes` | +35~45 | params.rs 同批 |
| `src/store.ts` | 改 | 图形状态、CRUD、`buildParams`、`applyPreset` 清理、`hasContent`、面板折叠状态 | +150~180 | types |
| `src/shapeGeom.ts` | **新建** | 理想轮廓折线化 / 命中测试 / 包围盒与端点换算（纯函数，便于测） | +120~160 | types |
| `src/components/PreviewPane.vue` | 改 | 图形模式、创建、命中、端点/8 向手柄、Esc、守卫 | +200~260 | store、shapeGeom |
| `src/components/ShapeList.vue` | **新建** | 列表（类型图标/页码/坐标/上下移层/删除/清空/🔍） | +120~150 | store |
| `src/components/ShapeProps.vue` | **新建** | 选中图形属性（类型/线宽/颜色/抖动/填充/页码） | +140~170 | store |
| `src/components/ParamsPanel.vue` | 改 | 新分组「⑤ 图形标注」+ 摘要 + 自动展开 | +25~35 | ShapeList/Props |
| `src/styles.css` | 改 | 图形选中轮廓 / 端点手柄 / 列表样式 | +35~45 | PreviewPane |
| `src/App.vue` | 改 | Esc 处理器扩展 | +5~10 | store |
| **合计（不含 shapes.rs）** | | | **≈ 900~1100** | |

> 前端没有任何单测基建（`package.json:13` 的 `test` 是 `cargo nextest run --workspace`），纯 TS 几何函数要么加 vitest（新依赖），要么只在 Rust 侧测对应图元。建议 `src/shapeGeom.ts` 保持**无 Vue 依赖的纯函数**，为将来加测试留口。

### 3.2 函数级清单

#### 3.2.1 `src/types.ts`（+35~45 行）

- 新增 `ShapeKind` 联合、`UiShape` 接口（§2.1）。
- `UiParams`（`:103-132`）加 `shapes: UiShape[]`（放在 `regions` 后，`:126`）。
- 注意：`UiShape.points` 用 `[number, number][]`，与后端 `Vec<[f32;2]>` 对齐；字段名全是单词，天然 camelCase/snake_case 一致。

#### 3.2.2 `src-tauri/src/params.rs`（+130~160 行）

1. `UiShape` 结构体（§2.1），`#[serde(rename_all="camelCase", default)]` + `PartialEq`。
2. `impl From<&Shape> for UiShape`（颜色 `format!("#{:02x}...")`，参考 `UiRegion` 的 `:291-293`）。
3. `to_handwriting_params()`：在 `:503-602` 区域映射旁新增 shapes 映射：
   - `fill: Option<String>` → `models::parse_color()`（参考 `:507-513`），`None` 时回退主墨色 `self.fill`（`HandwritingParams.fill`，`:608`）→ 写入 `ShapeSpec.fill: [u8;3]`；
   - 组装 `ShapeSpec`（`shapes.rs:76-101`），`page` 放在包装层（§2.3 契约）。
4. `from_handwriting_params()`（`:636-675`）：`shapes: p.shapes.iter().map(UiShape::from).collect()`；若预设永远不含图形，置空亦可，但映射更稳妥。
5. `UiParams::default()`（`:344-378`）补 `shapes: Vec::new()`。
6. 单测：`UiShape` 往返 + 非法颜色报错（照 `:834-866 test_invalid_color_reports_error` 的风格）。

#### 3.2.3 `src/store.ts`（+150~180 行，改 6~8 处）

新增状态（放在 `:428-440` 区域状态附近）：

```ts
// ---- 图形标注 ----
shapeMode: false,
shapeKind: "rect" as ShapeKind,     // 当前工具
shapes: [] as UiShape[],            // 与 regions 同级；数组序 = 叠放序（后画的在上）
editingShapeIndex: -1,              // 预览里的选中/可拖拽态
selectedShapeIndex: -1,             // 列表面板选中
shapeHoverIndex: -1,                // 列表悬浮 → 预览临时高亮
shapeFocusIndex: -1,
shapeFocusToken: 0,
```

新增函数（镜像区域那套，命名一一对应）：

| 函数 | 参照 | 说明 |
| --- | --- | --- |
| `setShapeMode(on)` | `setRegionMode`（`:2137-2141`） | 进入时 `regionMode=false`、`editingIndex=-1`；退出时清 `editingShapeIndex` |
| `setRegionMode(on)` 改 | `:2137-2141` | `on=true` 时同时 `shapeMode=false`、`editingShapeIndex=-1`（互斥） |
| `addShape(shape)` | `confirmRegionDialog`（`:2211-2241`） | 推入并选中，`scheduleRender()` |
| `updateShapeGeometry(index, patch)` | `updateRegionGeometry`（`:2334-2339`） | 只改几何，保留样式 |
| `updateShapeStyle(index, patch)` | — | 属性面板用；改样式也要重渲染 |
| `selectShape(index)` / `jumpToShape(index)` | `jumpToRegion`（`:2348-2357`） | 切页 + 进入编辑态 |
| `focusShape(index)` | `focusRegion`（`:2360-2366`） | `shapeFocusToken += 1` |
| `deleteSelectedShape()` / `clearShapes()` | `:2368-2385` | 删除后清索引 + 重渲染 |
| `cancelShapeEdit()` | `cancelRegionEdit`（`:2341-2345`） | 清三个索引 |
| `shapeLabel(s, i)` | `regionLabel`（`:2095-2112`） | `「↗ 箭头 第2页 (x,y w×h)」` |
| `clampShapeRect()`（或放 `shapeGeom.ts`） | `clampRect`（`:2116-2135`） | **返回 f32，不复用整数版** |

修改点：

- `buildParams()` `:989` 旁加 `shapes: store.shapes.map(s => ({...s, points: s.points?.map(p => [...p])}))`（深拷贝 points）。
- `hasContent()` `:626-632` 加 `|| store.shapes.length > 0`；`workflowMode()` `:637-640` 同理（否则纯图形被当空态，`PreviewPane.vue:689` 会被 QuickStart 盖住）。
- `applyPreset()` `:1639` 与导入清理点（`:898`、`:1479`、`:1515`、`:2378`）同步 `store.shapes.splice(0)` + 重置索引。
- `panelCollapseState`：`PanelCollapseState` 类型（`:74-79`）、`loadPanelCollapseState`（`:81-95`）各加 `shapes` 键。
- `presetParamSnapshot`（`:1658-1684`）**不要**加图形（与 regions 一样不算版式改动），但要在 UI 上让用户明白图形不随预设走（§2.4）。

#### 3.2.4 `src/components/PreviewPane.vue`（+200~260 行，核心）

**(a) 状态机与互斥**

```ts
type ShZone = "outside" | "move" | "tl"|"tr"|"bl"|"br"|"l"|"r"|"t"|"b" | "p0" | "p1" | "pN";
const shDragging = ref(false);          // 正在拖出新图形
const shAdjusting = ref(false);         // 正在调整已选图形
const shZone = ref<ShZone>("outside");
const shPress = reactive({ x: 0, y: 0 });
const shDraft = reactive({ x: 0, y: 0, w: 0, h: 0 });   // 显示像素
const shHasEdit = computed(() => store.editingShapeIndex >= 0 && ...);
```

互斥规则（建议直接做成矩阵，避免「框选文字」和「画图形」同时挂起）：

| 当前态 | 点击「框选文字」 | 点击「图形」 | 点空白 | Esc |
| --- | --- | --- | --- | --- |
| 普通 | 进入 regionMode | 进入 shapeMode | 清区域/图形选中 | 清选中 |
| regionMode | 退出 | 进 shapeMode（自动退 regionMode） | 取消选中 | 退出 regionMode（现状 `App.vue:126-133` 是「先清选中」，可保留） |
| shapeMode | 进 regionMode（自动退 shapeMode） | 切换图元类型（保持模式） | 取消草稿 | 退出 shapeMode |
| 图形调整中 | — | — | 提交/取消 | 取消调整（不提交） |

**(b) 指针事件优先级**（改 `onDown` `:402-451`）

```
1. 中键 → beginPan（现有 :406-410，不动）
2. 已选图形命中手柄/本体（shHasEdit）→ beginShapeAdjust
3. store.shapeMode → 开始拖出新图形（记 anchor，shDragging=true）
4. 区域编辑命中（现有 :420-428）
5. 区域框选（现有 :431-442）
6. 区域命中 → jumpToRegion（现有 :445-449）
7. 平移（现有 :450）
```

注意：**图形模式优先于区域**，因为二者互斥且进入模式是显式动作；但「图形调整」应优先于「新建图形」，否则选中后再点手柄会被当成拖新框（与区域现有优先级一致，`:419-428` 在 `:431` 之前）。

**(c) 创建手势（两类）**

| 图元 | 手势 | 落库 |
| --- | --- | --- |
| `rect/ellipse/triangle/brace_*/bracket_*` | 拖包围盒（可反向拖，`min/max` 归一化） | `x/y/w/h = toSrcRect(draft)` |
| `line/arrow/underline` | 拖起点→终点（方向有意义） | `points=[src(start), src(end)]`，`x/y/w/h = 归一化包围盒`（`w/h` 允许 0，见下） |
| `polyline` | 逐点点击、双击结束（完整版）；MVP 建议先「两点直线 + 可拖端点」或直接砍到 full | `points=[...]` |

- 阈值：沿用现有「拖动 <4 显示像素视为误触」（`:554-559`）。线类建议按**长度**判（`<8px` 忽略），避免点一下就生成零长线。
- 线类包围盒可能出现 `w=0`（垂直线）或 `h=0`（水平线）。POC 的 `normalized_bbox` 能接受（`shapes.rs:305-312`），但**不要把区域校验规则（`models.rs:654-672`：`w<=0||h<=0` 报错）照抄到图形**，否则垂直线永远校验失败。建议图形校验只拒绝 `w<0||h<0`（或直接交给 `normalized_bbox` 归一化）。
- 落库前用 f32（不要用 `clampRect` 的 `Math.round`，`store.ts:2125-2133`），否则细线在放大后会「跳 px」。

**(d) 命中测试（新建 `shapeHitAt`）**

```ts
function shapeHitAt(px: number, py: number, tol = 8): number {
  const f = factor.value;
  for (let i = store.shapes.length - 1; i >= 0; i--) {   // 数组序 = 叠放序，后画的在上
    const s = store.shapes[i];
    if (s.page !== store.pageIndex + 1) continue;        // 只命中当前页（参照 findRegionAt :250）
    if (hitShape(s, f, px, py, tol)) return i;
  }
  return -1;
}
```

`hitShape` 规则（`src/shapeGeom.ts`）：

- `line/arrow/underline`：点到线段距离 ≤ `max(tol, thickness*f/2 + 2)`。
- `polyline`：任一相邻两点线段命中。
- `rect/ellipse/triangle/brace_*/bracket_*/check`：**理想轮廓**折线化后做点-折线距离；`filled` 时额外做内部命中（`rect/ellipse/triangle`）。
- 细长图形（`min(w,h)*f < 3*tol`）退化为「包围盒内含点即命中」，否则细到没法点。
- 容差与 `zoneAt` 的 8px 保持一致（`:213`），并额外考虑手绘抖动会让墨迹偏离理想路径约 `jitter` px（`shapes.rs:94`）→ 实际容差建议 `8 + jitter`。

**为什么不用包围盒命中**：见 §1.2 的坑；空心圈注/大框会挡住其内部文字区域的点击（现有 `findRegionAt:255` 是内含点判定）。列表选择兜底仍然存在。

**(e) 手柄：端点 vs 8 向**

- 抽出纯函数 `zoneAtRect(rect, px, py, m=8)`（从 `zoneAt:209-230` 提取），区域与「包围盒类图形」共用，避免复制两份数学。
- 包围盒类图形：8 向手柄（四角 + 四边中点，视觉提示沿用 `HANDLE_DOTS` `:592-601`）。
- 线类（line/arrow/underline）：**两端点手柄**（直径 9px 圆点），`shZone = "p0"|"p1"`；拖动某端点只改该点（另一端不动），并重算包围盒。
- 折线：`p0..pN` 端点手柄；「增删中间点」放完整版。
- **细长图形降级**：当显示宽度或高度 < 24px 时，8 向手柄严重重叠（`min 8px` 框宽 vs 8px 容差，`:486-489`），建议只保留长短轴两端 2 个手柄（大括号/方括号几乎总是这种形态）。
- 大括号建议**只开放上下 + 右侧手柄**（禁止左右自由拉伸），否则「太直/太鼓」的问题会放大（`05-symbol-shapes.md:192-199`）。

**(f) 调整与落库**

- `beginShapeAdjust` 参照 `beginAdjust`（`:382-400`）：抓取基准几何到 `startBox`，`shAdjusting=true`。
- `applyShapeAdjust` 参照 `applyAdjust`（`:456-490`）：`move` 时整体平移（同时平移 `points`）；`tl/tr/...` 时改包围盒（f32，最小 8 显示像素）；`p0/p1` 时改端点并重算 bbox；全部钳制在 `[0, bgNatW/bgNatH]`（对应 `clampPt:283-289`，但用原始像素）。
- 松手才写回（照 `:536-552` 的 `changed` 判定），避免 `store.ts:1047` 的 300ms 防抖在拖动中被反复取消/触发。
- 写回后建议**立即 `doRender()` 一次**（`store.ts:1014`），否则用户要等 300ms 才看到墨迹；`PreviewPane.vue:714` 的「刷新」按钮就是直接调 `doRender`，可复用同款调用。

**(g) 守卫与取消**

- `onWheel` `:345` 的守卫加 `shAdjusting.value || shDragging.value`。
- `beginPan` `:356` 的守卫同上。
- `onMove` `:518-523` 后会接 `if (shAdjusting) applyShapeAdjust(p)`、`else if (shDragging) 更新 shDraft`。
- `onUp` `:526-561`：先处理 `shAdjusting`（写回），再处理 `shDragging`（新建），顺序不能反。
- `onCancel` `:563-568`：把 `shAdjusting/shDragging/shZone` 一并复位。
- `onBoxDown` `:580-585`：点击预览外侧灰底时 `cancelShapeEdit()`。
- `App.vue:124-135`：条件加 `store.editingShapeIndex >= 0 || store.selectedShapeIndex >= 0 || store.shapeMode`；动作改为「先取消草稿/选中，再退出模式」（建议封装 `cancelShapeEdit()` + `setShapeMode(false)`）。

**(h) 覆盖层（模板 `:624-655`）**

在 `.rubber-band` 之后、`.interact-layer` 之前插入：

```html
<!-- 图形选中/悬浮轮廓：理想几何（无抖动），虚线，pointer-events:none -->
<svg v-if="shapeOverlay" class="shape-overlay">
  <polyline :points="shapeOverlay.points" />
  <circle v-for="h in shapeOverlay.handles" ... />
</svg>
```

- `pointer-events:none`，样式仿 `.region-highlight`（`styles.css:125-132`）与 `.rubber-band`（`:134-147`）。
- 悬浮高亮（列表 hover）复用同一条渲染路径，颜色改成红色虚线（对齐 `highlightBox:163-172` 的语义）。
- 草稿层：拖拽中用 **理想轮廓 + 虚线包围盒**，松手后由引擎出图。

**(i) 工具栏入口（`:695-715`）**

`pagenav` 已是 6 个控件（`:696-714`），再加「模式开关 + 图元下拉」会挤。两个方案：

1. 紧凑：`NButton` 切换「✏️ 图形」+ `NPopselect` 选图元（naive-ui 已在 `App.vue:3` 用过 `NPopselect`）。
2. 两行：`pagenav` 下再加一行「工具条」，图形/区域两个 toggle + 图元条。推荐这个，视觉上更像「批注工具」。

#### 3.2.5 图层面板：新建 `ShapeList.vue`（不复用 `RegionList.vue`）

**结论：新建。** 理由：

- `RegionList.vue` 只有 127 行，但强耦合区域语义：`getRegionBadge` 走角色/高亮（`:20-41`）、`regionLabel`（`:69`）、`jumpToRegion/openEditRegionDialog/deleteSelectedRegion/clearRegions`（`:5-17`）、悬浮/单击/双击/🔍（`:50-79`）。要复用就得把「数据源 + 标签 + 四个动作」全部 prop 化，改造量 ≈ 重写，且容易碰坏现有区域列表。
- 图形列表需要：类型图标、页码、线宽/颜色色块、**上下移层**、删除、清空、🔍 聚焦——与区域只有 60% 重合。
- 可复用的只有 CSS：`.region-list{height:96px...}`（`styles.css:697-704`）、`.region-item`（`:706-729`）。建议新增 `.shape-list/.shape-item` 或直接复用这两个类名（改动最小）。
- 新增约 140 行，与 `RegionList.vue` 同构，长期更好维护。

**右栏位置：** `ParamsPanel.vue` 的 `NCollapse` 内、「④ 文字区域」（`:413-419`）之后新增「⑤ 图形标注」：

```vue
<NCollapseItem v-if="showShapes" name="shapes" title="图形标注">
  <template #header-extra><span class="collapse-count">{{ store.shapes.length }} 个</span></template>
  <ShapeList />
  <ShapeProps v-if="store.selectedShapeIndex >= 0" />
</NCollapseItem>
```

配套 4 处小改（容易漏）：

1. `GROUPS`（`ParamsPanel.vue:66`）加 `"shapes"`。
2. `collapse` 默认值（`:69-76`）加 `shapes: true`。
3. `PanelCollapseState` / `loadPanelCollapseState`（`store.ts:74-95`）加 `shapes`。
4. `showShapes`（仿 `showRegions:88` + 自动展开 watcher `:116-122`）：`store.shapes.length > 0 || store.shapeMode`。
5. `contentSummary`（`:101-106`）在「已创建 N 个文字区域」旁补图形数；否则用户不知道图形也算内容。

#### 3.2.6 属性面板控件（`ShapeProps.vue`）

| 控件 | naive-ui 组件 | 范围/默认 | 说明 |
| --- | --- | --- | --- |
| 类型 | `NSelect` | 12 种（扁平 kind） | 允许改类型（矩形→椭圆即改解释，几何不变） |
| 线宽 | `NSlider` + `NInputNumber` | 1~20 px，默认按纸张宽推算 `clamp(round(bgNatW/700), 2, 6)` | POC 默认 2.4（`shapes.rs:119`），A4 2480 宽下偏细 |
| 颜色 | `NColorPicker` + hex `NInput` | 默认「跟随主墨色」(`fill=null`) | 复用 `ParamsPanel.vue:251-266` 的写法 |
| 抖动 | `NSlider` | 0~6 px，默认 1.4 | **单位是 px**（`shapes.rs:94-95`），不是 0~1 |
| 填充 | `NSwitch`/`NCheckbox` | 仅 rect/ellipse/triangle 可用 | 默认关（09 文档风险表也建议默认空心） |
| 页码 | `NInputNumber` | 1..`previewPages.length` | 改页码 = 移动到该页，同时切到该页 |
| 层级 | 上移/下移按钮 | — | v1 用数组序，不引入 `z` 字段 |
| 删除 | `NButton` danger | — | 无撤销，建议删除前 `appDialog` 确认（参照 `importDocx` 的错误弹窗用法） |

`NSelect/NColorPicker/NInputNumber/NSlider/NCheckbox` 在 `ParamsPanel.vue:11-22` 已全部引入，无新依赖。

#### 3.2.7 `src/styles.css` / `src/App.vue`

- `styles.css`：`.shape-overlay`（绝对定位铺满 `.draw-area`、`pointer-events:none`）、`.shape-handle`（9px 圆点，仿 `.handle-dot:149-156`）、`.shape-highlight`（红色虚线，仿 `.region-highlight:125-132`）。约 +40 行。
- `App.vue`：仅 Esc 处理（`:124-135`），约 +6 行。

#### 3.2.8 core 侧交付接口（同事实现，此处只列前端依赖的契约）

1. `models.rs`：`Shapes` 挂在 `HandwritingParams`（`#[serde(default)]`）；**必须含 `page`**（多页文档 `background_pages` 已存在，`models.rs:489-492`）。
2. `engine.rs:71-168`：缩放 `x/y/w/h/points/thickness/jitter`（细节见 §3.4）。
3. `engine.rs:785-822`：每页合成时画当前页图形；顺序建议「主文字之后」（批注语义），且 `n_pages` 计算（`:776-781`）纳入图形的最大页码。
4. `engine.rs:304-... region_local_params`：`rp.shapes.clear()`。
5. `models.rs:583` 校验：`page>=1`、`thickness>=0`、`jitter>=0`、`w/h` 不做正数强校验（线类允许 0）。
6. `shape.rs` 契约调整（§2.3 表）：kind 扁平化、`ShapeSpec` 加 `page`（或外层包装）、`points` 保持页面像素。

### 3.3 依赖顺序与可并行拆分

```
(0) 契约冻结（0.5d，前后端一起）── 结论写进本文 §2.3 + 05 文档
      │
      ├─(1a) core：models/engine/validate（2d，同事）
      └─(1b) 前端：types.ts + params.rs + store 骨架（1d，可先用假数据）
             │
             ├─(2) PreviewPane 图形模式（2~2.5d）
             │      └─ 依赖 shapeGeom.ts（先写纯函数，可与 (2) 同人）
             ├─(3) ShapeList + ShapeProps + ParamsPanel 分组（1.5d，可并行）
             └─(4) 集成回归：预览/PNG/PDF/降采样/多页（1d，最后）
```

### 3.4 预览降采样时的图形几何缩放（`scaled_params_for`，`engine.rs:71-168`）

预览对宽 > 4096 的背景降采样（`engine.rs:46`、`:236-260`），并等比缩放所有空间参数（`:71-168`）。**图形必须加进这个循环**：

| 字段 | 是否缩放 | 说明 |
| --- | --- | --- |
| `x/y/w/h` | ✅ ×`scale` | 与区域一致（`:125-132`） |
| `points[i]` | ✅ ×`scale` | **POC 里 points 是页面绝对像素**（`shapes.rs:87-89`、`:195`），不像设计稿说的归一化坐标（`05:107-108`）——漏缩放会出现「预览位置对、导出错位」或反之 |
| `thickness` | ✅ ×`scale`，下限 0.8 | `shapes.rs:189` 已有 `.max(0.8)` 兜底，但仍要按比例缩，否则粗线在预览里偏粗 |
| `jitter` | ✅ ×`scale`（可为 0） | **最容易漏**：不缩的话，预览中的抖动幅度相对图形更大，「预览比导出更潦草」 |
| `seed` | ❌ | 同 seed 同抖动形态 |
| `page`/`filled`/`fill` | ❌ | 非空间参数 |

同步检查：`scaled_params_for` 在缓存命中（`:225`）与未命中（`:235`）两条路径都会调用，改一处即可覆盖。

**测试建议**：造一张 5000px 宽背景，同一组图形分别走 `render_pages` 与 `preview_pages_impl`，断言「图形包围盒按 `4096/5000` 等比」而不是像素相等（降采样后墨迹必然不同，现有 `preview_matches_export_with_same_seed` 测试（`engine.rs:1160`）用小背景，覆盖不到这条）。

### 3.5 导出 PNG / PDF 是否自动继承

**是，零改动自动继承。** 证据链：

| 出口 | 命令 | 引擎入口 | 是否经过 `generate_pages_with` |
| --- | --- | --- | --- |
| 预览 | `main.rs:110-160` → `engine::render_all_pages_preview`（`:119`） | `engine.rs:988-993` → `preview_pages_impl:933-965` | ✅（`:962`） |
| 导出 PNG | `main.rs:165-177` → `engine::export`（`:172`） | `engine.rs:983-985` → `save_all:863-909` | ✅（`save_all` → `render_pages:837-861` → `:858`） |
| 导出 PDF | `main.rs:182-186` → `engine::export_pdf`（`:185`） | `engine.rs:999-1004` → `render_pages`（`:1004`） | ✅ |

补充：

- PDF 是 **300 DPI 整页位图**（`engine.rs:995-998` 注释、`main.rs:179-180`），图形与文字一样烧成像素，不会出现「PDF 里线条变矢量/消失」的问题；代价是 PDF 体积随页面分辨率走（现状已如此）。
- 预览额外叠了「边界提示」（`main.rs:120-125 overlay_bounds`，仅预览、不进导出），图形不受影响。
- 只要图形画在 `generate_pages_with` 的每页循环里（`engine.rs:785-822`），三条出口天然一致 —— 这是本方案最大的工程优势。

### 3.6 交互状态图（简）

```
[idle] ──点「图形」──► [shapeMode] ──拖出──► [dragging] ──松手──► [selected]
   ▲                        │                                        │
   │                        └────Esc / 点空白────► [idle]            │
   │                                                                 ▼
   └──点空白 / Esc── [selected] ◄──命中图形/手柄── [adjusting] ◄──拖手柄──┘
```

---

## 4. 交互设计建议（5 条操作流）

**操作流 1｜圈住数字（椭圆）**
① 预览下方点「✏️ 图形」→ 图元选「椭圆」；② 在数字外拖一个略大于数字的框；③ 松手自动选中（8 向手柄出现，虚线轮廓）；④ 拖角微调、拖内部整体移动；⑤ 点空白退出。
要点：空心椭圆**只命中描边**（§3.2.4d），不会挡住圈内文字区域的双击编辑。

**操作流 2｜流程图连线（箭头/线）**
① 选「箭头」；② 从圆 A 边缘拖到圆 B 边缘（方向即箭头方向）；③ 松手后两个**端点手柄**出现，拖端点精修；④ 右侧属性面板调线宽 3px、颜色「跟随主墨色」、抖动 1.4。
要点：线类包围盒可能是 0 宽/0 高，手柄用端点而不是 8 向。

**操作流 3｜框住一段文字（矩形）**
① 选「矩形」；② 拖出框；③ 拖内部整体移动（move 区）、拖边/角缩放；④ 若压住正文，可在属性面板调「线宽 1px」减淡。
要点：矩形默认**空心**；`filled` 开关只在 rect/ellipse/triangle 出现，且默认关（避免大面积墨块压住底图）。

**操作流 4｜右侧大括号归组（brace_right）**
① 选「}」；② 拖出覆盖多行的高度（宽度窄）；③ 只开放上下手柄 + 右中手柄拉伸，避免拉宽变形；④ 属性面板可加大线宽/抖动。
要点：细长图形的手柄降级（§3.2.4e）；这是唯一建议**限制拉伸自由度**的图元。

**操作流 5｜列表管理与多页**
① 右栏「⑤ 图形标注」自动展开（首次出现时，仿 `ParamsPanel.vue:116-122`）；② 悬浮列表项 → 预览出现红色虚线轮廓（同 `highlightBox` 语义）；③ 单击 → 切页并选中；④ 双击/「🔍」→ `focusShape` 居中放大；⑤ 拖「↑/↓」调叠放；⑥「删除选中 / 清空」。
要点：列表是「空心图形点不中」时的兜底入口，必须有。

**键盘（建议，完整版）**：`Esc` 取消草稿/退出模式；`Delete` 删除选中；`←↑→↓` 微调 1px（`Shift` 10px）；`Ctrl+Z` 撤销（需要 store 级 undo，完整版）。

**不建议的手势**：「智能模式」按宽高比自动判断线/矩形（`05:218` 已否掉）——手写标注里横线、细矩形、下划线本来就难分，误判成本高于多点一次下拉。

---

## 5. 「符号字符面板」低成本替代方案评估

### 5.1 做法与成本（≈0.5~1.5 人日）

在正文工具栏（`TextEditorModal.vue:48-54`）加一排符号按钮（`→ ▶ □ ○ { } [ ] ✓ ✗`），点击插入到光标处。现有编辑器是 contenteditable，粘贴就是用 `document.execCommand("insertText", …)`（`ParaEditor.vue:848`），插入字符没有技术难度。

### 5.2 为什么「看起来能行、实际会静默失败」

字符最终走「测量宽度 → 光栅化」（`layout.rs:786-787`）。缺字链路是：

1. `font.rs:95` `let id = self.font.glyph_id(ch);` —— ab_glyph 对**字体里没有的字符**返回 `GlyphId(0)`（crates 源码 `ab_glyph-0.2.32/src/ttfp.rs:212-216`：`glyph_index(c).unwrap_or(0)`）。
2. `font.rs:97` `let Some(outline) = self.font.outline(id) else { ... return None }` —— 取 `.notdef`（glyph 0）的轮廓；若该字形没有有效轮廓（空白 `.notdef`），返回 `None`（`ttfp.rs:263-270` 会过滤空 bounds），并缓存 `None`（`font.rs:98-99`，注释就写着「缺字（tofu）时跳过」）。
3. `font.rs:127-129` `rasterize()`：`let Some(cached) = ... else { return; }` —— **静默返回：不画、不报错、不告警**。
4. 但宽度仍按 `.notdef` 的 advance 走：`font.rs:75-78 glyph_width()` 直接 `h_advance(glyph_id(ch))` —— 结果是「**留一个空格大小的空白洞**」；若字体的 `.notdef` advance 为 0，则**字距直接塌陷**（后一个字左移过来），比空白更难看。

也就是说：用户点一下 `▶`，界面毫无反馈，预览里要么空白、要么一个豆腐块（取决于字体有没有 `.notdef` 字形）。**没有 fallback，也没有「缺字」API 可查**——`font.rs` 目前只暴露 `glyph_width/ascent/rasterize`（`:74-150`），没有 `has_glyph()`。

### 5.3 现有 fallback 能力与限制

| 方案 | 可行性 | 限制 |
| --- | --- | --- |
| 直接插入 unicode 字符，用主手写字体 | ❌ 不可靠 | 手写字体多为裁剪子集；缺字静默消失（§5.2） |
| 插入**独立 TextRun** 并绑定系统符号字体（`TextRunStyle.font_path`，`types.ts:3-11`） | ✅ 可行 | layout 支持逐 run 字体解析：`layout.rs:516-526`（`font_path` 优先 → role → 主字体）；但要先把字符包进 run（编辑器里选择 → 「设为符号字体」），且符号是**印刷体**，与手写墨迹风格不一致 |
| 给 core 加「多字体 fallback 链」 | ⚠️ 成本高（core 改动，~2~3d） | 需要逐字符尝试多个 FontFace、改 `glyph_width`/`rasterize`/`layout` 三处；收益只是行内小符号，不值 |
| 加 `has_glyph(ch)` 查询 + 面板只显示可用符号 | ✅ 低成本兜底 | 需要 core 新增 1 个公有方法（`font.rs` 内 5 行），面板据此禁用缺失符号；仍解决不了「圈住数字/跨行连线/大括号」 |

### 5.4 字体覆盖不可知（关键约束）

仓库与发布包**不含任何字体**（`README.md:229-231`、`.gitignore` 的 `/fonts` 条目），字体由用户自备；同一符号在不同手写体里的覆盖情况完全不同。因此：

- 符号面板必须**运行时检测**（新增 `has_glyph`）或**只提供 ASCII 符号**（`→↑↓` 等 GB2312 常见符号覆盖较好，但 `▶` U+25B6 在不少中文字体里没有）；
- 无法在文档里给出「这些符号一定可用」的结论 —— 必须按用户字体实测。建议加一个 `#[test]` 或调试命令，用 3~5 款常见手写体跑一遍覆盖率（字体不入库，测试要 `skip`）。

### 5.5 结论

**只能作为补充，不能替代图形图层。** 它解决不了 issue 截图里的核心场景：

- 圈住数字（椭圆环绕）：字符 `○` 无法随内容缩放、无法手绘；
- 框住一段文字（矩形）：没有「任意尺寸的空心框」字符；
- 文字块之间连线 + 箭头：行内字符做不到跨行/任意角度；
- 右侧 `}` 归组多行：单个 `}` 字符的高度与行高绑定，无法覆盖 10 行；
- 竖线分隔栏：没有可任意拉长的 `|` 字形（用 `|` 只能是单行高度）。

若产品坚持要「行内小符号」体验，建议按 §5.3 第 2/4 条做（**绑定符号字体的 run + `has_glyph` 预检**），成本约 1~1.5 人日，明确标注「印刷体符号」，不要假装是手写。

---

## 6. 风险与工作量

### 6.1 风险表

| # | 风险 | 概率/影响 | 证据 | 对策 |
| --- | --- | --- | --- | --- |
| R1 | **契约不一致**（POC `ShapeSpec` vs 设计稿 5 处差异） | 高 / 高（返工） | `shapes.rs:50-101` vs `05:99-124` | 动工前冻结契约（§2.3），先改 core 的 kind 扁平化 + page，再写前端类型 |
| R2 | **预览/导出图形错位或大小不一** | 中 / 高（用户投诉） | `scaled_params_for:71-168` 目前不含 shapes | 缩放 x/y/w/h/points/thickness/jitter（§3.4）+ 5000px 宽背景回归测试 |
| R3 | **图形不持久化**，重启/切预设即丢 | 高 / 高（产品预期） | 预设白名单 `presets.rs:86-115`；无工程文件（§2.5） | v1 UI 明示；中期做工程文件（另计 1.5~2d） |
| R4 | 图形命中与文字区域抢手势 | 中 / 中 | 区域是包围盒命中 `PreviewPane.vue:245-260` | 图形用描边命中 + 阵列倒序 + 列表兜底（§3.2.4d） |
| R5 | 拖动时「旧墨迹 + 新虚线」重影 | 高 / 低（观感） | 预览是位图 `main.rs:138-150`；渲染防抖 300ms `store.ts:1047` | 松手立即 `doRender()`；文案提示；完整版可做前端矢量预览 |
| R6 | 大括号观感差 / 细长图形手柄重叠 | 中 / 中 | `shapes.rs:576-610`；手柄最小 8px `PreviewPane.vue:486-489` | POC 按 3~4 种高度调参（`05:199`）；细长图形降级为 2 手柄 |
| R7 | 纯图形页面被判为「空」 | 高 / 中（功能不可用） | `hasContent():626-632` 未计 shapes；`PreviewPane.vue:689` 用 QuickStart 盖住 | 同批改 `hasContent/workflowMode/contentSummary` |
| R8 | 性能：全页 bool 掩码 + 每图形光栅化 | 低 / 中 | `rasterize_shape` 接收页级掩码 `shapes.rs:170-180`；2480×3508 ≈ 8.7MB/页 | 单页 <50 图形无压力；必要时改包围盒局部掩码（引擎侧） |
| R9 | 前端无单测，几何回归靠手测 | 中 / 中 | `package.json:13` | `shapeGeom.ts` 写成纯函数；可选加 vitest（新依赖） |
| R10 | 预设脏标记/清理逻辑漏改 | 中 / 中 | `applyPreset:1639` 只清 regions；`store.ts:1479/1515/2378` 同 | 统一封装 `resetDocumentContent()` 一次清 regions+shapes |
| R11 | 符号方案缺字无提示 | 中 / 中 | `font.rs:127-129` 静默 return | 不主推；若要做得先加 `has_glyph`（§5.3） |
| R12 | 折线交互复杂度被低估 | 中 / 中 | 需要「逐点点击/双击结束/增删点」状态机 | MVP 先只做两点，折线放完整版；或提供「直线 + 端点拖」 |

### 6.2 工作量估算（POC / 可用 MVP / 完整）

> 口径：1 人日 = 6h 有效编码；含自测，不含跨人等待。范围含 core（同事）+ 命令层 + 前端。

| 阶段 | 交付内容 | 前端/命令层 | 引擎（core） | 合计 |
| --- | --- | --- | --- | --- |
| **POC**（已基本完成） | `shapes.rs` 12 类图元 + 单测 + 示例出图 | 0 | 0.5 | **0.5** |
| **可用 MVP** | 契约冻结 + `UiShape`/`UiParams` 映射 + 图形模式（拖建/选中/移动/缩放/端点）+ 列表/属性面板 + `buildParams`/清理/降采样 + PNG/PDF 回归 | 交互 2.0~2.5<br>面板 1.5<br>命令层 0.5<br>集成回归 1.0<br>**小计 5.0~5.5** | models/engine/validate<br>**2.0** | **7.0~7.5**（含 0.5~1d 缓冲 → **7~9**） |
| **完整版** | 以上 + 自由手绘、旋转、折线增删点、撤销/重做、图形模板、工程文件持久化、帮助/引导文案 | +4~6 | +0.5~1 | **11~14** |

**与 `05-symbol-shapes.md` 第 7 节的核对**：该表分项（含新增的「不加图形也会被带坏的三处」0.5d）为
0.5+2.0+0.5+2.5+1.5+0.5+1.0 = **8.5 人日**，结论已对齐为 **≈7~9 人日**；本文口径 7.0~7.5 + 0.5~1d 缓冲，两者一致。

### 6.3 非工作量但影响排期的决策

| 决策 | 选项 | 建议 | 影响 |
| --- | --- | --- | --- |
| `ShapeKind` 序列化形态 | 元组变体（现状） vs 12 个扁平单元变体 | **扁平化**（`brace_left`） | 不改的话 TS 联合类型丑陋，TS/预设都要额外解析 |
| 图形持久化 | 不进预设（现状语义） vs 新增工程文件 vs 塞进预设 | **v1 不进预设，另立工程文件需求** | 影响 R3 与 Python 互通承诺 |
| 折线范围 | MVP 两点 / 完整版多点多段 | MVP 只做「两点直线 + 端点拖」 | 少 1~1.5 人日 |
| `▶` 项目符号 | 图形层手摆 vs 排版层 bullet | v1 先允许手摆 + 在文档里说明；排版 bullet 另立需求 | 图形层做「行首符号」体验一般（要逐行对齐），不要承诺 |

---

## 7. 明确结论

### 7.1 能否实现 / 有无硬阻塞

**能实现，没有技术硬阻塞。** 逐项核对：

| 关注点 | 结论 | 证据 |
| --- | --- | --- |
| 坐标系 | 无阻塞。图形沿用「背景原始像素」，覆盖层用现有 `factor` 换算 | `PreviewPane.vue:98-101`、`types.ts:57` |
| 持久化 | **非技术阻塞，是产品决策**：预设只存版式（`presets.rs:86-115`），图形默认不持久化；要存需另做工程文件 | §2.4、§2.5 |
| 字体 | 无阻塞。图形是矢量掩码，**不依赖任何字体**，天然绕开 `font.rs:127-129` 的缺字静默跳过 | `shapes.rs:164-302` |
| PDF 导出 | 无阻塞。`export_pdf` 复用 `render_pages`，图形自动进 PDF | `engine.rs:999-1004` |
| 预览降采样 | 无阻塞，但**必做** `scaled_params_for` 缩放（含 points/jitter） | `engine.rs:71-168` |
| 多页文档 | 无阻塞，前提是持久化模型**补 `page`**（POC 目前没有） | `shapes.rs:76-101`、`models.rs:489-492` |
| 预览/导出像素级一致 | 小背景可保证；大背景降采样后只能保证**几何等比**，不保证逐像素 | `engine.rs:46`、测试 `:1160` |

### 7.2 三个必须先拍板的决策（不是技术问题）

1. **契约**：`ShapeKind` 改扁平 12 变体、`ShapeSpec` 加 `page`、`jitter` 明确 px、`points` 明确页面像素 —— 现在 POC 未提交，改成本最低；一旦前端 types.ts 按元组变体写死，返工 ≥1 人日。
2. **持久化**：图形不进预设（v1）还是引入工程文件；若不决策，按默认实现用户重启即丢，容易被当成 bug。
3. **折线/自由手绘**是否进 MVP：直接影响 1~2 人日与交互复杂度。

### 7.3 六类图形实现成本排序（含原因）

| 排名 | 图元 | 成本 | 主要成本在哪 |
| --- | --- | --- | --- |
| 1 | **大括号 `{ }`** | 最高（引擎 40~60 行 + 调参 0.5d） | 复合贝塞尔（上弧-尖角-下弧）+ 尖角朝向/镜像 + 「高度变了就太直/太鼓」的观感调参（`shapes.rs:576-610`、`05:190-199`）；前端还要限制拉伸自由度 |
| 2 | **折线 / 自由连线** | 高（**前端**最贵，+1~1.5d） | 几何本身最简单（`shapes.rs:207-215`），贵在交互：逐点点击、双击结束、N 个端点手柄、增删中间点、越界钳制 |
| 3 | **椭圆（手绘圈）** | 中高 | 几何已有 8 段贝塞尔 + 起收笔过冲（`shapes.rs:509-575`），贵在**观感调参**（圈注要「起笔压过收笔」才像手画），代码量不大 |
| 4 | **方括号 `[ ]`** | 中 | 几何 3 段折线很简单（`shapes.rs:249-267`），贵在**细长图形的 8 向手柄重叠**（`PreviewPane.vue:486-489`），需要手柄降级 |
| 5 | **箭头 →** | 中低 | 主线 + 两条箭头短线（`shapes.rs:330-365`），前端与线共用端点手柄；贵在箭头头部的手绘不对称调参 |
| 6 | **矩形 / 直线 / 竖线分隔** | 最低 | 纯包围盒/两点，POC 已稳定（`shapes.rs:199-229`） |
| — | **`▶` 实心三角** | 几何最低，**语义最高** | 画一个填充三角形是 10 行（`shapes.rs:231-247`），但截图里它是**行首项目符号**：用图形层意味着每行手动摆一个并对齐文字基线，用户会痛苦。真要解决应做「段落级 bullet」排版功能（另立需求），不要用图形层硬扛 |

> 一句话：**最贵的是大括号（几何+观感），其次是折线（交互），椭圆形贵在「像手画」而不是「画得出」，`▶` 贵在它不是图形问题而是排版问题。**

### 7.4 建议的下一步

1. 先把 §2.3 的契约表发给 core 同事，确认 `ShapeKind` 扁平化 + `page` 字段（0.5 人日内完成，POC 未提交是窗口期）。
2. 按 §3.3 的顺序排 MVP：命令层映射（1d）→ PreviewPane（2~2.5d）→ 面板（1.5d）→ 回归（1d）。
3. 同步修 3 个「不加图形也会被带坏」的点：`hasContent()`（`:626-632`）、`applyPreset` 清理（`:1639`）、`PanelCollapseState`（`:74-95`）。
4. 验收必须包含：>4096px 宽背景下的预览/导出对齐、多页文档第 2 页图形、PNG 与 PDF 三处一致（§附录 B）。

---

## 附录 A：逐文件改动锚点速查

| 文件 | 关键行 | 改动 |
| --- | --- | --- |
| `src/types.ts` | `:103-132` | `UiParams.shapes`；新增 `UiShape/ShapeKind` |
| `src/store.ts` | `:428-440`、`:626-632`、`:637-640`、`:942-996`（`:989`）、`:1639`、`:898`、`:1479`、`:1515`、`:2378`、`:74-95` | 状态、内容判定、buildParams、清理、折叠状态 |
| `src/components/PreviewPane.vue` | `:209-230`、`:245-260`、`:262-276`、`:342-363`、`:402-451`、`:456-490`、`:492-561`、`:563-585`、`:624-655`、`:695-715` | 手柄抽取、命中、图形模式、守卫、覆盖层、工具栏 |
| `src/components/ParamsPanel.vue` | `:64-77`、`:87-89`、`:101-106`、`:116-122`、`:413-419` | 新分组、摘要、自动展开 |
| `src/components/ShapeList.vue`（新） | — | 列表 + 上下移层 + 删除/清空 + 🔍 |
| `src/components/ShapeProps.vue`（新） | — | 属性控件 |
| `src/shapeGeom.ts`（新） | — | 轮廓折线化、命中、包围盒/端点换算 |
| `src/App.vue` | `:124-135` | Esc 扩展 |
| `src/styles.css` | `:93-156`、`:697-729` | 覆盖层/手柄/列表样式 |
| `src-tauri/src/params.rs` | `:178-235`（参照）、`:303-342`、`:344-378`、`:407-632`（`:503-602` 旁）、`:636-675` | `UiShape` + 映射 + Default |
| `src-tauri/src/main.rs` | `:54-61`、`:404-412` | **不改**（无新命令） |
| `crates/core/src/core/models.rs` | `:485-538`、`:583-672` | `shapes` 字段 + 校验（同事） |
| `crates/core/src/core/engine.rs` | `:71-168`、`:304`、`:776-822` | 缩放 + 合成 + 页数（同事） |
| `crates/core/src/core/shapes.rs` | `:47-101` | kind 扁平化 + `page`（同事，未提交） |

## 附录 B：验收 / 回归清单

1. **旧数据兼容**：无 `shapes` 的旧预设 JSON 能载入（`presets::load` 白名单，`presets.rs:118-182`）；旧 `UiParams`（无 shapes 字段）能被 Tauri 反序列化（`params.rs:304` 结构级 default）。
2. **坐标一致性**：同一图形在「适应窗口 / 400% 放大 / 平移后」三种视口下，DOM 轮廓与位图墨迹重合（误差 ≤ 2 显示像素）。
3. **降采样**：5000px 宽背景，预览图形与导出 PNG 的包围盒按 `4096/5000` 等比（不是像素相等）。
4. **多页**：文档导入 3 页，图形放在第 2 页，预览第 2 页可见、PNG/PDF 第 2 页可见，第 1/3 页不可见。
5. **三出口一致**：预览 / 导出 PNG（`main.rs:172`）/ 导出 PDF（`main.rs:185`）同一 seed 下图形位置与线宽一致。
6. **互斥**：图形模式与框选模式不能同时挂起；Esc 各状态下行为符合 §3.2.4b 矩阵。
7. **内容判定**：只有图形（无文字/区域/文档）时，预览不被 `<QuickStart>` 盖住（`PreviewPane.vue:689`）。
8. **清理**：应用预设 / 导入 docx / 导入文档后，`shapes` 清空且不残留在新纸上。
9. **性能**：单页 50 个图形时，拖动结束后 `doRender` 耗时可接受（与纯文字渲染同一量级，±30%）。
10. **不破坏现状**：`cargo nextest run --workspace` 全绿（含 `preview_matches_export_with_same_seed`、`presets` 往返、`params.rs` 颜色校验）。
