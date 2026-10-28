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
