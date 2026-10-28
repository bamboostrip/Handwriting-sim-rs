# 右侧操作栏重排实施计划（阶段分组 + 折叠 + 统一导入）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将右侧参数面板重组为 6 个按使用阶段排序的折叠分组，统一 4 个导入入口，高级参数默认折叠且状态本地记忆——只改布局与展示时机，不改任何功能逻辑。

**Architecture:** 纯前端改造（Vue 3 + Naive UI）。`store.ts` 新增折叠状态 localStorage 持久化与选区跟踪状态；`ParamsPanel.vue` 用 `NCollapse` 重组现有模板片段；`App.vue` 状态栏接管主题切换；`ParaEditor.vue` 上报选区状态驱动「选区角色」条按需显示。

**Tech Stack:** Vue 3 `<script setup>` + naive-ui 2.43（NCollapse/NCollapseItem）+ Vite。

**设计文档:** `docs/superpowers/specs/2026-10-28-right-panel-reorganization-design.md`（本计划唯一需求来源）

## Global Constraints

- **零功能改动**：所有按钮/输入框/弹窗的行为、`store` 业务逻辑、后端 Rust 命令一律不动，只做模板搬移与展示条件。
- **永远不要提交 `src-tauri/Cargo.toml`**（换行符噪音）；`git add` 只加任务里明确列出的文件。
- **本机 shell 不能 `cd`**：cargo/pnpm/git 全部从仓库根 `D:\AllCode\rust\handwrite-sim` 直接执行，git 用 `git -C "D:\AllCode\rust\handwrite-sim"`。
- **验证命令**（每个任务的"测试循环"，前端无单测设施）：`npx vue-tsc --noEmit`（期望无输出）+ `pnpm build`（期望 `✓ built in ...`；chunk >500kB 警告是既有噪音，不算失败）。
- **代码风格**：中文注释；提交信息 conventional + 中文描述（feat/fix/docs + scope）。
- **保活规则**（设计文档 §4.3）：①text ②regions ③roles 三组 `display-directive="show"`；④import ⑤sim ⑥preset 用默认 `if`。
- **localStorage 键名**：`handwritesim_panel_collapse`（与现有 `handwritesim_theme_preference` 等键命名风格一致）。

---

### Task 1: store.ts 折叠状态持久化 + 选区跟踪状态

**Files:**
- Modify: `src/store.ts`（三处小改：常量区 ~line 20、函数区 ~line 49 后、store reactive 对象 line 371 后）

**Interfaces:**
- Produces: `loadPanelCollapseState(): PanelCollapseState | null`（null = 首次使用无记录）
- Produces: `savePanelCollapseState(s: PanelCollapseState): void`
- Produces: `type PanelCollapseState = { text: boolean; regions: boolean; roles: boolean; import: boolean; sim: boolean; preset: boolean }`（true = 折叠）
- Produces: `store.hasTextSelection: boolean`（响应式，编辑器内存在非折叠文字选区时为 true）

- [ ] **Step 1: 加 localStorage 键常量**

在 `src/store.ts` 顶部常量区（`const KEY_THEME_PREFERENCE = "handwritesim_theme_preference";` 之后一行）加：

```ts
const KEY_PANEL_COLLAPSE = "handwritesim_panel_collapse";
```

- [ ] **Step 2: 加折叠状态读写函数**

在 `getSkippedVersion()` 函数结束后（约 line 49 的 `}` 之后）插入：

```ts
/** 右栏 6 个折叠组的展开记录（true = 折叠）；无记录返回 null（首次使用） */
export type PanelCollapseState = {
  text: boolean;
  regions: boolean;
  roles: boolean;
  import: boolean;
  sim: boolean;
  preset: boolean;
};

export function loadPanelCollapseState(): PanelCollapseState | null {
  try {
    const raw = localStorage.getItem(KEY_PANEL_COLLAPSE);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<PanelCollapseState>;
    return {
      text: v.text ?? false,
      regions: v.regions ?? false,
      roles: v.roles ?? true,
      import: v.import ?? false,
      sim: v.sim ?? true,
      preset: v.preset ?? true,
    };
  } catch {
    return null;
  }
}

export function savePanelCollapseState(s: PanelCollapseState): void {
  try {
    localStorage.setItem(KEY_PANEL_COLLAPSE, JSON.stringify(s));
  } catch {
    // localStorage 不可用时静默降级为不记忆
  }
}
```

- [ ] **Step 3: store 加选区状态字段**

在 `src/store.ts:371`（`paraStatus: "光标定位到段落后可用按钮设置格式",` 之后一行）加：

```ts
  /** 编辑器内存在非折叠文字选区（驱动右栏「选区角色」标记条按需显示） */
  hasTextSelection: false,
```

- [ ] **Step 4: 验证编译**

Run: `npx vue-tsc --noEmit && pnpm build`
Expected: vue-tsc 无输出；build 出现 `✓ built in ...`

- [ ] **Step 5: Commit**

```bash
git -C "D:\AllCode\rust\handwrite-sim" add src/store.ts
git -C "D:\AllCode\rust\handwrite-sim" commit -m "feat(ui): 右栏折叠状态持久化与选区跟踪状态（store）"
```

---

### Task 2: ParaEditor 上报文字选区状态

**Files:**
- Modify: `src/components/ParaEditor.vue:960-977`（`onSelectionChange` 函数）

**Interfaces:**
- Consumes: `store.hasTextSelection`（Task 1 已加）
- Produces: `store.hasTextSelection` 随选区实时更新（编辑器内选中文字 → true；选区取消/移出 → false）

- [ ] **Step 1: 修改 onSelectionChange**

把 `src/components/ParaEditor.vue` 中现有的：

```ts
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
```

整体替换为：

```ts
function onSelectionChange(): void {
  if (
    document.activeElement === editorRef.value ||
    editorRef.value?.contains(document.activeElement) ||
    (window.getSelection()?.rangeCount &&
      editorRef.value?.contains(window.getSelection()!.getRangeAt(0).commonAncestorContainer))
  ) {
    const sel = window.getSelection();
    const selInEditor =
      !!sel &&
      sel.rangeCount > 0 &&
      !!editorRef.value?.contains(sel.getRangeAt(0).commonAncestorContainer);
    // 非折叠选区（真实选中了一段文字）才让右栏「选区角色」标记条出现
    store.hasTextSelection = !!selInEditor && !sel!.isCollapsed;
    updateCurrentParaFromSelection();
    updateFloatingBubble();
  } else {
    floatingVisible.value = false;
    store.hasTextSelection = false;
  }
}
```

（`store` 已在该文件 import，无需新增导入。）

- [ ] **Step 2: 验证编译**

Run: `npx vue-tsc --noEmit && pnpm build`
Expected: 同 Global Constraints。

- [ ] **Step 3: Commit**

```bash
git -C "D:\AllCode\rust\handwrite-sim" add src/components/ParaEditor.vue
git -C "D:\AllCode\rust\handwrite-sim" commit -m "feat(ui): 编辑器上报文字选区状态"
```

---

### Task 3: 主题切换移入底部状态栏

**Files:**
- Modify: `src/App.vue`（script imports、themeOptions 常量、status-actions 模板 ~line 130-139）
- Modify: `src/styles.css`（`.status-bar` 区块后加一条按钮样式）

**Interfaces:**
- Consumes: `store.themePreference`、`setThemePreference`、`cycleThemePreference`、`isDarkActive`（均已有）
- Produces: 状态栏「使用教程」左侧出现三态主题按钮（hover 弹选择、点击轮换）——与旧面板按钮行为一致

- [ ] **Step 1: App.vue script 增补**

`src/App.vue` 的 naive-ui import 行改为：

```ts
import { darkTheme, NButton, NConfigProvider, NDialogProvider, NMessageProvider, NPopselect } from "naive-ui";
```

store import 列表中加入 `cycleThemePreference`、`setThemePreference`（`isDarkActive`、`store` 已有）。

import 语句之后加常量：

```ts
const themeOptions = [
  { label: "🖥️ 自动跟随系统", value: "auto" },
  { label: "☀️ 浅色模式", value: "light" },
  { label: "🌙 深色模式", value: "dark" },
];
```

- [ ] **Step 2: 状态栏模板加按钮**

把 `<div class="status-actions">` 内容改为（📖 使用教程前插入主题按钮）：

```vue
<div class="status-actions">
  <NPopselect
    :value="store.themePreference"
    :options="themeOptions"
    @update:value="(val: any) => setThemePreference(val)"
    trigger="hover"
  >
    <NButton size="tiny" quaternary class="status-theme-btn" @click="cycleThemePreference()">
      <template v-if="store.themePreference === 'auto'">
        🖥️ 自动 ({{ isDarkActive() ? '深色' : '浅色' }})
      </template>
      <template v-else-if="store.themePreference === 'dark'">🌙 深色</template>
      <template v-else>☀️ 浅色</template>
    </NButton>
  </NPopselect>
  <span class="status-sep">·</span>
  <span class="status-link" @click="openHelpModal()" title="查看软件使用教程与技巧">
    📖 使用教程
  </span>
  <span class="status-sep">·</span>
  <span class="status-link" @click="openAboutModal()" title="查看软件版本与开源项目">
    v{{ APP_VERSION }} · 关于与更新
  </span>
</div>
```

- [ ] **Step 3: styles.css 加按钮适配**

`.status-bar .status-text` 规则之后加：

```css
.status-bar .status-theme-btn {
  height: 20px;
  padding: 0 6px;
  font-size: 11.5px;
  color: var(--text-sub);
}
```

- [ ] **Step 4: 验证编译**

Run: `npx vue-tsc --noEmit && pnpm build`
Expected: 同 Global Constraints。

- [ ] **Step 5: Commit**

```bash
git -C "D:\AllCode\rust\handwrite-sim" add src/App.vue src/styles.css
git -C "D:\AllCode\rust\handwrite-sim" commit -m "feat(ui): 主题切换移入底部状态栏"
```

---

### Task 4: ParamsPanel 六组折叠重排 + 统一导入入口

**Files:**
- Modify: `src/components/ParamsPanel.vue`（整文件模板重组，本计划核心任务）
- Modify: `src/components/RegionList.vue`（去掉内部标题、三行提示并一行）
- Modify: `src/styles.css`（折叠面板与导入条目样式）

**Interfaces:**
- Consumes: `loadPanelCollapseState` / `savePanelCollapseState` / `PanelCollapseState` / `store.hasTextSelection`（Task 1）、`store.hasTextSelection` 实时更新（Task 2）、状态栏主题按钮（Task 3——本任务删除面板旧按钮后由状态栏接管）
- Produces: 右栏最终形态——6 组 NCollapse（text/regions/roles/import/sim/preset），分组名即 `expandedNames` 中的字符串

- [ ] **Step 1: 重写 ParamsPanel.vue 的 `<script setup>`**

将整个 `<script setup lang="ts">` 块替换为：

```vue
<script setup lang="ts">
//! 右侧参数面板：按使用阶段分 6 组折叠（文本内容 / 文字区域 / 笔迹角色 /
//! 导入素材 / 笔迹仿真与版式 / 预设）；docx 正文、打印底图、背景图片、字体
//! 四个导入入口统一收进「导入素材」组；底部固定「预览 / 导出 / 导出 PDF」。
//! 折叠状态经 localStorage 记忆（store.loadPanelCollapseState）。

import { computed, reactive, watch } from "vue";
import {
  NButton,
  NCheckbox,
  NCollapse,
  NCollapseItem,
  NColorPicker,
  NInput,
  NInputNumber,
  NSelect,
  NSlider,
} from "naive-ui";
import FontPicker from "./FontPicker.vue";
import ParaEditor from "./ParaEditor.vue";
import RegionList from "./RegionList.vue";
import RegionDialog from "./RegionDialog.vue";
import RoleManager from "./RoleManager.vue";
import {
  applyRoleToSelection,
  chooseBackground,
  doRender,
  exportFiles,
  exportPdf,
  getRoleBadgeInfo,
  importDocx,
  importDocument,
  isDarkActive,
  loadPanelCollapseState,
  loadPresetFromDialog,
  savePanelCollapseState,
  savePresetToDialog,
  selectPreset,
  setAlign,
  store,
  toggleIndent,
} from "../store";

const presetOptions = computed(() =>
  store.presets.map((p) => ({ label: p.name, value: p.path })),
);

// ---- 分组折叠状态：localStorage 记忆；首次使用按默认策略 ----
type PanelGroup = "text" | "regions" | "roles" | "import" | "sim" | "preset";
const GROUPS: PanelGroup[] = ["text", "regions", "roles", "import", "sim", "preset"];

const storedCollapse = loadPanelCollapseState();
const collapse = reactive<Record<PanelGroup, boolean>>(
  storedCollapse ?? {
    text: false,                          // 文本内容默认展开
    regions: store.regions.length === 0,  // 已有区域则展开，空则折叠
    roles: true,                          // 笔迹角色默认折叠
    import: false,                        // 导入素材默认展开
    sim: true,                            // 仿真参数默认折叠
    preset: true,                         // 预设默认折叠
  },
);
watch(collapse, () => savePanelCollapseState({ ...collapse }));

const expandedNames = computed<Array<string | number>>({
  get: () => GROUPS.filter((g) => !collapse[g]),
  set: (vals) => {
    const open = new Set(vals.map(String));
    for (const g of GROUPS) collapse[g] = !open.has(g);
  },
});

const regionCount = computed(() => store.regions.length);
const roleCount = computed(() => store.roles.length);
</script>
```

- [ ] **Step 2: 重写 ParamsPanel.vue 的 `<template>`**

将整个 `<template>` 块替换为（所有控件均为旧模板原样搬移，仅位置变化）：

```vue
<template>
  <aside class="params-col">
    <div class="params-scroll">
      <NCollapse v-model:expanded-names="expandedNames" class="panel-collapse">
        <!-- ============ ① 文本内容 ============ -->
        <NCollapseItem name="text" title="文本内容" display-directive="show">
          <template #header-extra>
            <span class="collapse-count">{{ store.paragraphs.length }} 段</span>
          </template>
          <div class="field-row" style="flex-wrap: wrap">
            <NButton size="tiny" @click="setAlign(0)">左对齐</NButton>
            <NButton size="tiny" @click="setAlign(1)">居中</NButton>
            <NButton size="tiny" @click="setAlign(2)">右对齐</NButton>
            <NButton size="tiny" @click="toggleIndent(true)">首行缩进</NButton>
            <NButton size="tiny" @click="toggleIndent(false)">取消缩进</NButton>
          </div>
          <div
            v-if="store.hasTextSelection"
            class="field-row role-tag-bar"
            style="flex-wrap: wrap; gap: 4px; margin-top: 2px"
          >
            <span class="tag-bar-label">选区角色:</span>
            <button
              v-for="role in store.roles.filter((r) => r.id > 0)"
              :key="role.id"
              type="button"
              class="role-tag-btn"
              :style="{
                backgroundColor: getRoleBadgeInfo(role, isDarkActive()).bg,
                color: getRoleBadgeInfo(role, isDarkActive()).color,
                borderColor: getRoleBadgeInfo(role, isDarkActive()).color,
              }"
              :title="`选区标记为：${role.name}`"
              @mousedown.prevent
              @click="applyRoleToSelection(role.id)"
            >
              {{ getRoleBadgeInfo(role, isDarkActive()).icon }} {{ role.name }}
            </button>
            <NButton size="tiny" tertiary @mousedown.prevent @click="applyRoleToSelection(0)">
              ✕ 清除标记
            </NButton>
          </div>
          <div class="hint-line">{{ store.paraStatus }}</div>
          <ParaEditor />
        </NCollapseItem>

        <!-- ============ ② 文字区域（手写 / 打印混排） ============ -->
        <NCollapseItem name="regions" title="文字区域" display-directive="show">
          <template #header-extra>
            <span class="collapse-count">{{ regionCount }} 个</span>
          </template>
          <RegionList />
        </NCollapseItem>

        <!-- ============ ③ 笔迹角色 ============ -->
        <NCollapseItem name="roles" title="笔迹角色" display-directive="show">
          <template #header-extra>
            <span class="collapse-count">{{ roleCount }} 个</span>
          </template>
          <RoleManager />
        </NCollapseItem>

        <!-- ============ ④ 导入素材（统一入口） ============ -->
        <NCollapseItem name="import" title="导入素材">
          <div class="import-entry">
            <div class="import-entry-head">
              <NButton size="small" type="primary" secondary @click="importDocx()">
                导入 Word 正文文本 (.docx)
              </NButton>
            </div>
            <div class="import-entry-desc">导入为待处理文本，保留文档中的高亮 / 角色标记</div>
          </div>

          <div class="import-entry">
            <div class="import-entry-head">
              <NButton size="small" type="primary" secondary @click="importDocument()">
                导入 PDF / Word 打印底图
              </NButton>
            </div>
            <div class="import-entry-desc">逐页作为背景打印底图，可选「识别高亮填空」或「纯底图」</div>
            <div v-if="store.docStatus" class="hint-line">📄 {{ store.docStatus }}</div>
          </div>

          <div class="field-row" style="margin-top: 8px">
            <span class="field-label">背景图片</span>
            <NInput
              v-model:value="store.backgroundPath"
              size="small"
              placeholder="未选择背景（png/jpg/webp/bmp）"
            />
            <NButton size="small" @click="chooseBackground()">选择</NButton>
          </div>
          <div class="field-row">
            <span class="field-label">字体</span>
            <FontPicker v-model:value="store.fontPath" placeholder="未选择字体（.ttf/.ttc/.otf）" style="flex: 1" />
          </div>
        </NCollapseItem>

        <!-- ============ ⑤ 笔迹仿真与版式 ============ -->
        <NCollapseItem name="sim" title="笔迹仿真与版式">
          <div class="field-row">
            <span class="field-label">文字颜色</span>
            <NColorPicker
              v-model:value="store.fontColor"
              :show-alpha="false"
              size="small"
              :swatches="['#000000', '#1a1a8c', '#8b0000', '#003366']"
              style="width: 120px"
            />
            <NInput v-model:value="store.fontColor" size="small" placeholder="#RRGGBB" style="width: 96px" />
          </div>

          <div class="group-card">
            <span class="group-legend">排版参数</span>
            <div class="sigma-grid">
              <span></span>
              <span class="col-head">数值</span>
              <span></span>
              <span class="col-head">随机扰动</span>

              <span class="field-label" style="width: auto">字水平间距</span>
              <NInputNumber v-model:value="store.wordSpacing" size="small" :min="0" :max="100" :show-button="false" />
              <span></span>
              <NInputNumber v-model:value="store.wordSpacingSigma" size="small" :min="0" :max="20" :show-button="false" />

              <span class="field-label" style="width: auto">字竖直间距</span>
              <NInputNumber v-model:value="store.lineSpacing" size="small" :min="0" :max="200" :show-button="false" />
              <span></span>
              <NInputNumber v-model:value="store.lineSpacingSigma" size="small" :min="0" :max="20" :show-button="false" />

              <span class="field-label" style="width: auto">字体大小</span>
              <NInputNumber v-model:value="store.fontSize" size="small" :min="8" :max="200" :show-button="false" />
              <span></span>
              <NInputNumber v-model:value="store.fontSizeSigma" size="small" :min="0" :max="20" :show-button="false" />
            </div>
          </div>

          <div class="group-card">
            <span class="group-legend">笔画扰动</span>
            <div class="sigma-grid" style="grid-template-columns: 84px 1fr">
              <span class="field-label" style="width: auto">水平笔画位移</span>
              <NInputNumber v-model:value="store.perturbX" size="small" :min="0" :max="20" :show-button="false" />
              <span class="field-label" style="width: auto">竖直笔画位移</span>
              <NInputNumber v-model:value="store.perturbY" size="small" :min="0" :max="20" :show-button="false" />
              <span class="field-label" style="width: auto">笔画旋转</span>
              <NInput
                v-model:value="store.perturbThetaText"
                size="small"
                placeholder="0.05"
                style="text-align: center"
              />
            </div>
          </div>

          <div class="group-card">
            <span class="group-legend">写错字</span>
            <div class="field-row">
              <span class="field-label">错字率</span>
              <NSlider v-model:value="store.miswriteRate" :min="0" :max="30" :step="0.1" style="flex: 1" />
              <span style="width: 48px; text-align: center">{{ store.miswriteRate.toFixed(1) }}%</span>
            </div>
            <div class="field-row">
              <span class="field-label">重写方式</span>
              <NSelect
                v-model:value="store.miswriteModeIndex"
                size="small"
                :options="[
                  { label: '右上方重写', value: 0 },
                  { label: '后文重写', value: 1 },
                ]"
              />
            </div>
            <div class="field-row">
              <span class="field-label">涂改方式</span>
              <NSelect
                v-model:value="store.strikeoutStyleIndex"
                size="small"
                :options="[
                  { label: '单横线', value: 0 },
                  { label: '双横线', value: 1 },
                  { label: '斜线', value: 2 },
                  { label: '叉号', value: 3 },
                ]"
              />
            </div>
          </div>

          <div class="group-card">
            <span class="group-legend">边距</span>
            <div class="margin-grid">
              <span></span>
              <NInputNumber v-model:value="store.marginTop" size="small" :min="0" :max="3000" :show-button="false" />
              <span></span>

              <NInputNumber v-model:value="store.marginLeft" size="small" :min="0" :max="3000" :show-button="false" />
              <span class="margin-center-mark">边距</span>
              <NInputNumber v-model:value="store.marginRight" size="small" :min="0" :max="3000" :show-button="false" />

              <span></span>
              <NInputNumber v-model:value="store.marginBottom" size="small" :min="0" :max="3000" :show-button="false" />
              <span></span>
            </div>
            <div class="field-row" style="justify-content: center; margin-bottom: 0; margin-top: 8px">
              <NCheckbox v-model:checked="store.boundsVisible">边界提示(仅预览)</NCheckbox>
              <NColorPicker
                v-model:value="store.boundsColor"
                :show-alpha="false"
                size="small"
                style="width: 72px"
              />
            </div>
          </div>
        </NCollapseItem>

        <!-- ============ ⑥ 预设 ============ -->
        <NCollapseItem name="preset" title="预设">
          <div class="field-row" style="margin-bottom: 0">
            <NSelect
              size="small"
              filterable
              placeholder="— 选择预设 —"
              :options="presetOptions"
              :value="null"
              @update:value="(v: string) => selectPreset(v)"
              style="flex: 1"
            />
            <NButton size="small" @click="loadPresetFromDialog()">载入</NButton>
            <NButton size="small" @click="savePresetToDialog()">保存</NButton>
          </div>
        </NCollapseItem>
      </NCollapse>

      <div class="hint-line" style="text-align: left">提示：参数改动会自动防抖渲染，也可手动点「预览」。</div>
    </div>

    <!-- ============ 主按钮行 ============ -->
    <div class="action-bar">
      <NButton type="primary" :loading="store.rendering" @click="doRender()">预览</NButton>
      <NButton type="primary" secondary @click="exportFiles()">导出</NButton>
      <NButton type="primary" secondary @click="exportPdf()">导出 PDF</NButton>
    </div>

    <RegionDialog />
  </aside>
</template>
```

**与旧模板的差异清单**（审查对照用）：
1. 对齐行移除「导入 docx」按钮（移入 ④ 组）。
2. 选区角色条加 `v-if="store.hasTextSelection"`。
3. 标题行的主题按钮、「ℹ️ 关于 / 检查更新」按钮删除（状态栏已接管）。
4. 「待处理文本」标题行整体删除（折叠头 `title="文本内容"` 替代）。
5. 字体/背景/文档底图三行移入 ④ 组，前两个入口改造为「按钮 + 一句话说明」的 import-entry 结构，`docStatus` 只读输入框压缩为 `v-if` 状态行。
6. 文字颜色移入 ⑤ 组顶部。
7. 预设行移入 ⑥ 组。
8. script 的 import 变化：删除 `NPopselect`、`NTooltip`、`cycleThemePreference`、`setThemePreference`、`openAboutModal`、`themeOptions` 常量（均不再使用）；新增 `NCollapse`、`NCollapseItem`、`loadPanelCollapseState`、`savePanelCollapseState`。`loadPresetFromDialog` / `savePresetToDialog` 保留（⑥ 组仍用）。

- [ ] **Step 3: RegionList.vue 去重与提示精简**

删除 `<div class="section-title">文字区域（手写 / 打印混排）</div>`（折叠头已承载标题）；把原三行 hint（框选说明 ×2 + 💡 角色联动提示）替换为一行：

```vue
<div class="hint-line">
  在左侧预览勾选「框选文字」后拖出矩形生成文字；单击列表项调整位置，双击重新编辑文字与参数。
</div>
```

其余部分（region-list、role-dot、删除选中/清空按钮）原样保留。

- [ ] **Step 4: styles.css 加折叠面板样式**

`.action-bar` 规则之前（右栏样式区块内）加：

```css
/* ---------------- 折叠分组面板 ---------------- */
.panel-collapse .n-collapse-item {
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--card-bg);
  margin-bottom: 8px;
}

.panel-collapse .n-collapse-item__header {
  font-weight: 700;
  font-size: 13px;
}

.panel-collapse .n-collapse-item__content-inner {
  padding: 4px 12px 10px;
}

.collapse-count {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-sub);
}

.import-entry {
  margin-bottom: 10px;
}

.import-entry-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.import-entry-desc {
  font-size: 11px;
  color: var(--text-sub);
  line-height: 1.5;
  margin-top: 3px;
}
```

- [ ] **Step 5: 验证编译**

Run: `npx vue-tsc --noEmit && pnpm build`
Expected: 同 Global Constraints。若报未使用导入（如 `NTooltip` 残留），按 Step 2 差异清单第 8 条修 import 后重跑。

- [ ] **Step 6: Commit**

```bash
git -C "D:\AllCode\rust\handwrite-sim" add src/components/ParamsPanel.vue src/components/RegionList.vue src/styles.css
git -C "D:\AllCode\rust\handwrite-sim" commit -m "feat(ui): 右栏重排为六组折叠+统一导入入口"
```

---

### Task 5: E2E 布局验证 + CHANGELOG（主会话执行，不分派子代理）

**Files:**
- Modify: `CHANGELOG.md`（`## [Unreleased]` 下新增 `### Changed` 小节）

**Interfaces:**
- Consumes: Task 1-4 全部成果
- Produces: 验证结论（截图留档 `docs/screenshots/`）+ 变更日志条目

**注意：** `src/api.ts` 是纯 Tauri IPC，浏览器直跑 `pnpm dev` 时渲染/文件对话框必然报错——这是预期噪音，本任务只验证**布局与展示时机**，不验证需要后端的行为。

- [ ] **Step 1: 起本地页面并逐项核对**

Run（主会话后台）: `pnpm dev`（vite 默认 5173 端口），浏览器打开核对以下清单（对应设计文档 §7.2）：

1. 6 组折叠头齐全：文本内容 / 文字区域(N 个) / 笔迹角色(N 个) / 导入素材 / 笔迹仿真与版式 / 预设。
2. 默认状态：text+import 展开，roles/sim/preset 折叠；regions 跟随区域数（无后端时恒为 0 个+折叠）。
3. 任意切换几组展开/折叠 → 刷新页面 → 状态保持（localStorage `handwritesim_panel_collapse`）。
4. text 组折叠再展开 → 编辑器内容仍在、可输入。
5. ④ 组两个导入按钮 + 说明 + 背景图片 + 字体四项齐全。
6. 状态栏主题按钮三态切换正常（浅/深/自动），面板标题行旧按钮已不在。
7. 选区角色条：编辑器无选区时隐藏；用鼠标选中一段文字后出现（角色数据为 store 默认值）。
8. 深浅两主题下折叠头、计数、组卡片边框颜色正常（无对比度异常）。

- [ ] **Step 2: 截图留档**

浅色+深色各截一张右栏全貌，存 `docs/screenshots/`（命名 `panel-reorg-light.png` / `panel-reorg-dark.png`）。

- [ ] **Step 3: 更新 CHANGELOG**

`## [Unreleased]` 下（在 `### Added` 之前）插入：

```md
### Changed

- **右侧操作栏重排**：按使用阶段分 6 组折叠（文本内容 / 文字区域 / 笔迹角色 / 导入素材 / 笔迹仿真与版式 / 预设），高级参数默认折叠、折叠头带计数、展开状态本地记忆；docx 正文、PDF/Word 打印底图、背景图片、字体四个导入入口统一收进「导入素材」组并配语义说明；主题切换移至底部状态栏（移除与状态栏重复的「关于」按钮）；「选区角色」标记条仅在编辑器选中文字时显示。纯布局调整，功能与数据流不变。
```

- [ ] **Step 4: Commit**

```bash
git -C "D:\AllCode\rust\handwrite-sim" add CHANGELOG.md docs/screenshots/panel-reorg-light.png docs/screenshots/panel-reorg-dark.png
git -C "D:\AllCode\rust\handwrite-sim" commit -m "docs(changelog): 右栏重排变更记录与截图"
```

---

## 收尾（全部任务完成后，主会话）

1. `cargo nextest run -p handwrite-sim` 复跑（后端虽无改动，防意外）。
2. 按记忆工作流：requesting-code-review 全分支终审 → finishing-a-development-branch 向用户呈现 4 选项（合并/PR/保留/丢弃——**勿自行合并**）。
