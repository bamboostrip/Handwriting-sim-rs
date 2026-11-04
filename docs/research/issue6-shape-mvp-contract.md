# issue #6 图形标注 MVP：冻结契约（实施基线）

> **状态：已按本契约实现（MVP）**。实现结果见 `docs/05-symbol-shapes.md` §6；
> 已知取舍（跨色叠放、折线 UI、持久化）见 §10。
>
> 本文件是**并行实施的唯一契约来源**。core / 命令层 / 前端三方都按此实现；
> 有疑问先找 Lead，不要单方面改契约。
> 设计依据：`docs/05-symbol-shapes.md`（可行性 + 设计）、
> `docs/research/issue6-ui-feasibility.md`（前端逐文件清单）。
> 分支：`feat/symbol-shapes`。**不要切分支、不要 git commit/push。**

## 0. MVP 范围（做什么 / 不做什么）

- 做：直线、箭头、矩形、椭圆、三角形、大括号（左/右）、方括号（左/右）、勾、下划线，共 12 种 id。
- 做：在预览图上拖拽创建、点选、8 向手柄缩放、整体拖动、右侧属性面板、图形列表、删除、叠放上下移、按页、给文字区域一键加框。
- **不做**：折线（core 保留能力但 UI 不暴露）、自由手绘、旋转、撤销重做、图形库、图形随预设保存。

## 1. 核心数据（`crates/core/src/core/shapes.rs`）

`ShapeKind` 保持 POC 现状（含元组变体，渲染分支清晰）：

```rust
pub enum ShapeKind {
    Line, Arrow, Polyline, Rect, Ellipse, Triangle,
    Brace(BraceSide), BraceSide = { Left, Right },
    Bracket(BracketSide), BracketSide = { Left, Right },
    Check, Underline,
}
```

`ShapeSpec` 相对 POC 的 2 处变更（其余字段不动）：

```rust
pub struct ShapeSpec {
    pub kind: ShapeKind,
    #[serde(default = "default_page")] pub page: i32,   // 新增：1 基页码，默认 1
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
    #[serde(default)] pub points: Vec<[f32; 2]>,
    pub thickness: f32,
    #[serde(default)] pub fill: Option<[u8; 3]>,        // 变更：None = 跟随主墨色
    pub jitter: f32,
    pub seed: u64,
    #[serde(default)] pub filled: bool,
}
```

新增稳定 id 映射（命令层与前端都依赖它，**字符串必须逐字一致**）：

| id | 变体 |
| --- | --- |
| `"line"` | Line |
| `"arrow"` | Arrow |
| `"polyline"` | Polyline |
| `"rect"` | Rect |
| `"ellipse"` | Ellipse |
| `"triangle"` | Triangle |
| `"brace_left"` | Brace(Left) |
| `"brace_right"` | Brace(Right) |
| `"bracket_left"` | Bracket(Left) |
| `"bracket_right"` | Bracket(Right) |
| `"check"` | Check |
| `"underline"` | Underline |

```rust
impl ShapeKind {
    pub fn as_id(&self) -> &'static str;          // 上表
    pub fn from_id(id: &str) -> Option<Self>;     // 上表
}
```

`ShapeSpec::with_fill([u8;3])` 改为写 `Some(...)`（构造器签名不变，POC 示例不受影响）。

## 2. 参数与渲染（`crates/core/src/core/models.rs` + `engine.rs`）

`HandwritingParams` 新增（`regions` 旁边）：

```rust
#[serde(default)] pub shapes: Vec<ShapeSpec>,
```

- **校验**（`validate_with`）：`page >= 1`；`x/y/w/h/thickness/jitter` 必须是有限值；`thickness >= 0`。
  新错误变体（中文信息，风格对齐现有 `ParamsError`）：
  `ShapePage { index }`、`ShapeGeometry { index }`、`ShapeThickness { index }`。
- **渲染**（`generate_pages_with`，`engine.rs:785-822` 页循环，主文字之后、`out.push` 之前）：
  1. 取本页图形 `shape.page.max(1) as usize - 1 == page_index`；
  2. 按**解析后的墨色**分组：`fill = shape.fill.unwrap_or(params.fill)`；每种颜色一个页面级 `Vec<bool>` 掩码；
  3. `shapes::rasterize_shape(&spec_scaled_to_page, &mut mask, width, height, 0, 0)`；
  4. 每种颜色一次
     `perturb::perturb_styled_layer_into(&mask, width, height, &PerturbStyle::new(color, false, 0.0, 0.0, 0.0), perturb_rng, &mut canvas)`
     ——σ=0 会走 `draw_printed_mask` 直写（图形自身已带手绘抖动，不能再整条旋转，也不消耗 rng，保持同 seed 稳定）。
- **页数**：`n_pages` 计算（`engine.rs:777-781`）追加 `max(shape.page)`，否则「只有图形的页」会丢。
- **预览降采样**（`scaled_params_for`，`engine.rs:71-168`）：对每个 shape 的 `x/y/w/h/thickness/jitter` 和
  `points[*][0..1]` 乘 `scale`；`page/fill/filled/seed` 不动。`thickness` 缩后 `< 1.0` 时按 1.0 兜底。
- `has_renderable_content` / 字体加载**不需要**包含图形（图形不依赖字体）。
- 既有行为零回归：`cargo test -p handwrite-sim` 必须全绿（当前 181 单元 + 5 集成）。

## 3. 命令层镜像（`src-tauri/src/params.rs` + `src/types.ts`）

前端 ↔ 命令层统一 camelCase（与 `UiRegion` 同风格），**逐字一致**：

```ts
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

// UiParams 新增
shapes: UiShape[];
```

Rust 侧 `UiShape`：`#[serde(rename_all = "camelCase", default)]`，`kind: String`，`fill: Option<String>`，
`points: Vec<[f32; 2]>`，其余同名；`UiParams` 结构已有 `#[serde(default)]`，`Default` 实现要补 `shapes: Vec::new()`；
`UiParams::from_handwriting_params` / `to_handwriting_params` 各加一条映射。
`"#RRGGBB"` 解析复用 `models::parse_color`；解析失败按 `None`（跟随主墨色）处理，不整体报错。
`kind` 无法识别时**跳过该图形**（不画错东西），其余图形照常。

## 4. 前端 store API（`src/store.ts` 导出，UI 只依赖这些）

状态：

```ts
store.shapeMode: boolean          // 图形标注模式（与 regionMode 互斥）
store.shapeKind: UiShapeKind      // 当前要画的图元，默认 "rect"
store.shapes: UiShape[]           // 全量图形（跨页，按数组顺序 = 绘制顺序）
store.selectedShapeIndex: number  // -1 = 未选中
```

函数（坐标一律**背景原始像素**，与 regions 一致）：

```ts
setShapeMode(on: boolean): void                  // 开：关 regionMode、清选中；关：清选中
setShapeKind(k: UiShapeKind): void
addShapeFromRect(kind: UiShapeKind, rect: [number, number, number, number],
                 points?: [number, number][]): number   // rect=[x0,y0,x1,y1] 可反向，内部归一化；返回新索引
addShapeAroundRegion(index: number): void        // 用 store.regions[index] 的矩形生成 Rect（一键加框）
shapeAtPoint(srcX: number, srcY: number): number // 命中测试（仅当前页），返回索引或 -1；细线按距离判定
updateShapeGeometry(index: number, rect: [number, number, number, number]): void // points 按比例跟随缩放
updateShapeStyle(index: number, patch: Partial<UiShape>): void
selectShape(index: number): void
clearShapeSelection(): void
deleteShape(index: number): void
clearShapes(): void
moveShapeOrder(index: number, dir: -1 | 1): void // 数组内上/下移 = 叠放前后
shapeLabel(s: UiShape, i: number): string        // 列表文案，如 "1. 矩形 第1页 2.4px"
```

联动（**必做**，否则功能会"看起来可用但丢数据/串状态"）：

- `buildParams()`：带上 `shapes: store.shapes.map(s => ({ ...s, points: s.points?.map(p => [...p]) }))`。
- `hasContent()` / `workflowMode()`：把 `store.shapes.length > 0` 计入（纯图形页不能显示空状态卡）。
- `applyPreset()`（`store.ts:1639` 附近）：清 `shapes`、退出 `shapeMode`、清选中（与 regions 同步）。
- 其它任何 `regions.splice(0)` / 重置入口：同步清 shapes。
- `PanelCollapseState`（`store.ts:74-95`）：加 `shapes: boolean` 折叠状态键（缺省 true=展开）。
- 新增图形时 `seed` 用 `Math.floor(Math.random() * 2 ** 32)` 生成。
- 创建失败（rect 太小，如宽高都 < 2px）：`addShapeFromRect` 返回 -1，不改 store。

## 5. UI 交互（`PreviewPane.vue` + 新面板组件）

- 底部工具栏（`PreviewPane.vue:695-715` 的 `pagenav`）：在「框选文字」旁加「✎ 标注」切换按钮；
  进入模式后再加一个图元下拉（10 种，中文名），以及「＋ 给文字区域加框」。
- 预览图上：
  - `shapeMode` 下 `pointerdown` 开始拖拽草稿（十字光标），`pointermove` 更新虚线框（线/箭头同时画一条预览线 + 箭头头），`pointerup` 调 `addShapeFromRect`；
  - **线/箭头必须把拖拽的起止点传进 `points`**（否则反向拖拽会丢方向）；
  - 非 `shapeMode` 下点击：先 `shapeAtPoint` 命中图形则选中并进入 8 向手柄调整态（复用现有 `zoneAt`/`beginAdjust`/`applyAdjust` 的路子）；
  - 选中图形显示蓝色虚线包围框 + 8 个手柄；拖内部整体移动，拖手柄缩放，松手写回 `updateShapeGeometry`；
  - `Esc` / 点空白：清选中 / 退出图形模式（与现有区域语义一致）。
- 右侧新增分组「④ 图形标注」（`ParamsPanel.vue`），内有：
  - 图形列表（新组件 `ShapeList.vue`）：单击定位（切到该页并选中）、双击改属性、🗑 删除、↑↓ 叠放；
  - 属性（新组件 `ShapeProps.vue`）：类型下拉、线宽（1~8px）、抖动（0~4px）、颜色（`n-color-picker`，含「跟随主墨色」开关）、填充开关（仅 rect/ellipse/triangle 显示）、页码（只读展示，创建时取当前页）。
- 样式：`PreviewPane.vue` 可在文件末尾加 `<style scoped>`；`ShapeList.vue`/`ShapeProps.vue` 用 scoped；
  **只有 PreviewPane 负责人可以往 `src/styles.css` 追加**（末尾单独区块，别改既有规则）。

## 6. 验收（Lead 会跑）

1. `cargo test -p handwrite-sim` 全绿（含新增 shapes 引擎集成测试）。
2. `cargo check --workspace` 通过（命令层编译）。
3. `pnpm build`（= `vue-tsc --noEmit && vite build`）通过。
4. 端到端：`UiParams`（含 12 种图形）→ `to_handwriting_params` → 引擎渲染 → PNG，
   图形出现在正确页码、正确位置、颜色跟随/覆盖均生效；预览降采样后位置仍对得上。
