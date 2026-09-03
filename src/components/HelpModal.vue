<script setup lang="ts">
//! 使用教程与功能指南弹窗。
//!
//! 详细介绍软件的核心功能、操作流程、PDF/DOCX 文档底图背景高亮识别填空、
//! 选框调整交互、多角色笔迹混排以及逼真度调节技巧。

import {
  NAlert,
  NButton,
  NModal,
  NScrollbar,
  NTabPane,
  NTabs,
  NTag,
} from "naive-ui";
import { closeHelpModal, store } from "../store";
</script>

<template>
  <NModal
    v-model:show="store.helpModalOpen"
    preset="card"
    title="手写模拟器 · 使用指南与功能教程"
    style="width: 720px; max-width: 95vw"
    :mask-closable="true"
    @update:show="(val: boolean) => !val && closeHelpModal()"
  >
    <div class="help-modal-content">
      <NTabs type="line" animated>
        <!-- Tab 1: 快速上手 -->
        <NTabPane name="quickstart" tab="🚀 快速上手">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <div class="guide-lead">
                欢迎使用手写模拟器！只需简单三步，即可将排版文本转换为充满真实人手书写随机感的高质量图片或 PDF。
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 1</div>
                <div class="step-body">
                  <div class="step-title">选择字体与纸张背景</div>
                  <div class="step-text">
                    在右侧参数面板中点击<strong>「手写字体」</strong>选择您喜欢的字迹（支持 <code>.ttf</code> / <code>.otf</code> / <code>.ttc</code>）；点击<strong>「背景信纸」</strong>选择纸张底图（支持纯白、作文格子纸、高校实验报告纸等多种内置素材）。
                  </div>
                </div>
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 2</div>
                <div class="step-body">
                  <div class="step-title">输入或导入文本</div>
                  <div class="step-text">
                    在<strong>「主文本与段落排版」</strong>输入框中键入文字，支持回车自然换行、段首缩进与对齐调整。您也可以点击<strong>「导入 docx」</strong>一键提取 Word 文档内容。停止输入约 300 毫秒后，左侧预览区将自动完成全分辨率实时排版。
                  </div>
                </div>
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 3</div>
                <div class="step-body">
                  <div class="step-title">实时预览与导出</div>
                  <div class="step-text">
                    左侧预览区展示最终视觉效果（可翻页查看长文档）。满意后在面板底部点击<strong>「导出为图片」</strong>批量保存 PNG，或点击<strong>「导出为 PDF」</strong>生成<strong>300 DPI 印刷级无损位图层 PDF</strong>，可直接打印存档。
                  </div>
                </div>
              </div>

              <NAlert type="info" title="常用快捷键与操作" style="margin-top: 12px">
                <ul class="guide-list">
                  <li><strong>Esc 键</strong>：若当前处于区域选框调整态，按 Esc 可立即取消选框并恢复纯净预览。</li>
                  <li><strong>翻页与底色</strong>：预览区下方可点击「上一页」「下一页」切换页面，点击「预览底色」切换视口对比度。</li>
                  <li><strong>自动防抖</strong>：任意参数调节均防抖 300ms 后后台异步渲染，不卡顿界面。</li>
                </ul>
              </NAlert>
            </div>
          </NScrollbar>
        </NTabPane>

        <!-- Tab 2: 文档底图与高亮填空 -->
        <NTabPane name="highlight" tab="📑 试卷/表格高亮填空">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <NAlert type="success" title="核心特色：彻底解决试卷、表格、实验报告格式错位问题" style="margin-bottom: 12px">
                复杂的试卷题目、带表格的实验报告如果全部重新排版，极易发生字体变样、行距表格跑偏。通过<strong>「高亮背景填空模式」</strong>，未标记题干原样保留为高清单页底图，仅将标记的高亮文字提取为手写体，实现天衣无缝的真实填空效果！
              </NAlert>

              <div class="feature-section">
                <div class="section-title">第一步：在 Word / 文档中高亮标记手写部分</div>
                <div class="guide-text">
                  打开您的原始 Word（.docx）文件：
                  <ul class="guide-list">
                    <li>需要保持打印字体的题目、表格、题干等：<strong>保持无背景高亮</strong>。</li>
                    <li>需要模拟手写填空的内容（如姓名、学号、填空题答案、论述题手写内容）：选中文字，点击 Word 工具栏的<strong>「突出显示文本颜色（高亮色）」</strong>涂上背景色（如黄色、绿色、青色等）。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">第二步：导出为 PDF 导入（强烈推荐 ⭐）</div>
                <div class="guide-text">
                  在 Word 中点击「文件」→「另存为 PDF」或「导出为 PDF」。<br />
                  然后在软件中点击<strong>「导入文档底图」</strong>并选择该 PDF 文件。<br />
                  <NTag size="small" type="warning" style="margin-top: 6px">💡 为什么推荐 PDF？</NTag>
                  Word 文档直接解析受本机字体环境和排版引擎差异影响，个别版式可能轻微偏移；而 PDF 具备固化的矢量排版和嵌入字形，本软件能 <strong>100% 精确像素级还原文档每一页底图</strong>。
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">第三步：自动智能识别与生成</div>
                <div class="guide-text">
                  导入时，软件会自动执行以下智能处理：
                  <ol class="guide-list">
                    <li><strong>自动擦除高亮背景色</strong>：底图上被高亮涂抹的底色块会被无痕擦除并恢复为洁净的纸张底色；</li>
                    <li><strong>精准定位与字号校准</strong>：通过全角字符紧包围盒度量算法，智能校准原文字号与行距，并在对应位置自动创建手写区域；</li>
                    <li><strong>自动绑定笔迹角色</strong>：不同颜色高亮会自动划分归属角色（如黄色归为手写角色 1，绿色归为手写角色 2）。</li>
                  </ol>
                </div>
              </div>
            </div>
          </NScrollbar>
        </NTabPane>

        <!-- Tab 3: 区域微调与笔迹角色 -->
        <NTabPane name="region" tab="✍️ 选框微调与多角色混排">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <div class="feature-section">
                <div class="section-title">1. 选框的激活、调整与退出</div>
                <div class="guide-text">
                  当您需要对自动识别的手写区域进行微调时：
                  <ul class="guide-list">
                    <li><strong>唤出选框</strong>：在右侧「文字区域」列表中单击某一条目，或者<strong>直接在左侧预览图纸上单击该文字</strong>，即可在该区域上方显示带有 8 个白色手柄的蓝色选框。</li>
                    <li><strong>平移位置</strong>：鼠标放在选框内部拖动，可整体平移框选位置。</li>
                    <li><strong>八向缩放</strong>：鼠标移到选框四角或四边手柄上（光标显示为双向箭头），按住拖动可任意扩大或缩小范围。</li>
                    <li><strong>取消选框（展示纯净预览）</strong>：<strong>在选框外侧的任意图纸空白处，或外侧灰色背景留白处单机鼠标</strong>（或按键盘 <code>Esc</code> 键），选框与手柄会立刻隐藏，恢复纯净清爽的原版预览。</li>
                    <li><strong>双击编辑属性</strong>：双击预览图上的区域或列表项，可打开详细对话框，修改文字、微调字号、单独覆盖字体或边距。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">2. 笔迹角色管理（多人书写与多字体混排）</div>
                <div class="guide-text">
                  在参数面板中展开<strong>「笔迹角色管理」</strong>：
                  <ul class="guide-list">
                    <li>系统默认为角色 0（主手写体）与角色 1（默认打印体）；导入带高亮的文档后，会自动为各高亮颜色生成角色（如手写角色 1、角色 2）。</li>
                    <li>您可以为每个角色单独设置<strong>不同的手写字体文件</strong>、<strong>墨水颜色</strong>（如黑色中性笔、蓝色圆珠笔、红笔批改）以及<strong>专属扰动强度</strong>。</li>
                    <li>修改角色属性后，所有关联该角色的手写区域会自动同步更新，轻松实现两人共写、批改痕迹或不同章节字迹风格混排！</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">3. 手动框选文字区域</div>
                <div class="guide-text">
                  除自动导入识别外，在任何普通信纸背景上，只要点击翻页栏上的<strong>「框选文字」</strong>按钮（光标变为十字线），在预览图上按住鼠标左键拉出一个矩形框，松开即可弹出区域创建对话框，随意添加手写或打印区域。
                </div>
              </div>
            </div>
          </NScrollbar>
        </NTabPane>

        <!-- Tab 4: 逼真度与排版技巧 -->
        <NTabPane name="tips" tab="💡 真实感调优与错字模拟">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <div class="feature-section">
                <div class="section-title">1. 排版参数与高斯扰动推荐经验值</div>
                <div class="guide-text">
                  人手书写最大的特征是<strong>行距不完全均匀、字距微小浮动、个别字形微倾斜</strong>：
                  <div class="param-table">
                    <div class="param-row">
                      <span class="param-name">字号 (font_size)</span>
                      <span class="param-desc">常规 A4 纸推荐 32~42 px（扰动 σ 设为 1.5~2.5）</span>
                    </div>
                    <div class="param-row">
                      <span class="param-name">字水平间距 (word_spacing)</span>
                      <span class="param-desc">推荐 3~8 px（扰动 σ 设为 1.5~2.5）</span>
                    </div>
                    <div class="param-row">
                      <span class="param-name">行距 (line_spacing)</span>
                      <span class="param-desc">常规文本推荐 40~52 px；若使用带横线的纸张，请调节至与背景横线对齐（扰动 σ 建议较小，如 0.8~1.8，防止严重压线）</span>
                    </div>
                    <div class="param-row">
                      <span class="param-name">笔画位移扰动 (perturb_x/y)</span>
                      <span class="param-desc">推荐 1.5~2.5 px，轻微打散呆板的电脑字形结构</span>
                    </div>
                    <div class="param-row">
                      <span class="param-name">笔画旋转扰动 (perturb_theta)</span>
                      <span class="param-desc">推荐 0.03~0.06 rad（约 2~3 度），手写笔画自然倾斜</span>
                    </div>
                  </div>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">2. 写错字涂改模拟（绝妙真实细节）</div>
                <div class="guide-text">
                  展开面板底部的<strong>「写错字模拟」</strong>：
                  <ul class="guide-list">
                    <li><strong>错字率</strong>：建议设置为 <strong>2% ~ 5%</strong>。数值过大显得潦草，轻微错字更具手写真实感。</li>
                    <li><strong>涂改样式</strong>：支持<strong>单线划掉</strong>、<strong>双线划掉</strong>、<strong>斜线涂划</strong>、<strong>叉号划除</strong>。涂抹笔迹采用贝塞尔曲线与随机粗细模拟真实笔画。</li>
                    <li><strong>重写模式</strong>：可选择「上方重写」（在被划掉错字的正上方用稍小字号补写正确字，最贴合课堂与考试书写习惯）或「后文重写」。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">3. 常见问题 FAQ</div>
                <div class="guide-text">
                  <ul class="guide-list">
                    <li><strong>Q: 软件没有自带手写字体怎么办？</strong><br />
                      由于多数艺术手写字体受商用版权保护，发布包不随附商业字体。您可以在系统字体选择器中选择系统自带字体，或者下载开源免费可商用的手写体（如霞鹜文楷 LXGW WenKai、沐瑶随心体等）。
                    </li>
                    <li><strong>Q: 打印导出后字迹模糊吗？</strong><br />
                      导出的 PNG 图片以及 PDF 均为高分辨率（PDF 内置 300 DPI 无损压缩图像层），直接在 A4 打印机上打印观感极其细腻逼真，无任何像素锯齿。
                    </li>
                  </ul>
                </div>
              </div>
            </div>
          </NScrollbar>
        </NTabPane>
      </NTabs>
    </div>

    <template #footer>
      <div style="display: flex; justify-content: flex-end">
        <NButton type="primary" size="medium" @click="closeHelpModal()">
          我知道了
        </NButton>
      </div>
    </template>
  </NModal>
</template>

<style scoped>
.help-modal-content {
  margin: -6px 0;
}

.help-pane {
  padding: 6px 12px 14px 4px;
}

.guide-lead {
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text-main);
  margin-bottom: 14px;
}

.step-card {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  background: var(--bg-item, rgba(0, 0, 0, 0.02));
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px 14px;
  margin-bottom: 10px;
}

.step-badge {
  background: var(--accent);
  color: #fff;
  font-size: 12px;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 12px;
  flex-shrink: 0;
  margin-top: 1px;
}

.step-body {
  flex: 1;
}

.step-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-main);
  margin-bottom: 4px;
}

.step-text {
  font-size: 12.5px;
  line-height: 1.55;
  color: var(--text-sub);
}

.feature-section {
  margin-bottom: 16px;
}

.section-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-main);
  margin-bottom: 6px;
}

.guide-text {
  font-size: 13px;
  line-height: 1.65;
  color: var(--text-main);
}

.guide-list {
  margin: 6px 0 0 18px;
  padding: 0;
  font-size: 12.5px;
  line-height: 1.6;
  color: var(--text-sub);
}

.guide-list li {
  margin-bottom: 4px;
}

.param-table {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 8px;
}

.param-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  padding: 6px 10px;
  border-radius: 6px;
  background: var(--bg-item, rgba(0, 0, 0, 0.02));
  border: 1px solid var(--border);
  font-size: 12px;
}

.param-name {
  font-weight: 600;
  color: var(--accent);
  min-width: 140px;
  font-family: monospace;
}

.param-desc {
  flex: 1;
  color: var(--text-sub);
  line-height: 1.45;
}

code {
  font-family: monospace;
  background: rgba(125, 125, 125, 0.12);
  padding: 1px 4px;
  border-radius: 3px;
  font-size: 0.9em;
}
</style>
