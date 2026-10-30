<script setup lang="ts">
//! 使用教程与功能指南弹窗。
//!
//! 详细介绍软件的核心功能、操作流程、PDF/DOCX 文档底图背景高亮识别填空、
//! 选框调整交互、多角色笔迹混排、逼真度调节技巧，
//! 以及预设系统、纯背景预览、深色模式等更多实用功能。

import {
  NAlert,
  NButton,
  NModal,
  NScrollbar,
  NTabPane,
  NTabs,
  NTag,
} from "naive-ui";
import { closeHelpModal, openOnboarding, store } from "../store";

function restartOnboarding(): void {
  closeHelpModal();
  openOnboarding();
}
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
      <NTabs type="line" animated v-model:value="store.helpTab">
        <!-- Tab 1: 快速上手 -->
        <NTabPane name="quickstart" tab="🚀 三分钟上手">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <div class="guide-lead">
                第一次用？先选一个开始方式：<strong>写文字</strong>、<strong>填试卷 / 表格</strong>，或者
                <strong>先看示例</strong>。字体和纸张会自动准备好，不需要先配一堆参数。
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 1</div>
                <div class="step-body">
                  <div class="step-title">选字迹与纸张（或直接选一套方案）</div>
                  <div class="step-text">
                    右栏顶部的<strong>「当前方案」</strong>可以一键载入「字体 + 纸张 + 版式」组合；
                    想单独换字迹，展开<strong>「② 字迹」</strong>选择字体（支持 <code>.ttf</code> / <code>.ttc</code> / <code>.otf</code>）。
                    软件不内置商业字体，首次会自动选用系统楷体；想更像手写，推荐换成自己的手写体。
                  </div>
                </div>
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 2</div>
                <div class="step-body">
                  <div class="step-title">放入内容：文字或文档底图</div>
                  <div class="step-text">
                    · <strong>纯文字</strong>：点「① 内容 → 写文字 / 编辑正文」，粘贴或输入文字，回车分段，选中文字还能指定笔迹角色。<br />
                    · <strong>Word 正文</strong>：点「导入 Word 正文 (.docx)」，自动提取段落、对齐与缩进。<br />
                    · <strong>试卷 / 表格</strong>：在 Word 里把要手写的部分用<strong>高亮</strong>标出，另存为 PDF 后点「导入 PDF / Word 打印底图」，
                    软件会自动擦除高亮并识别成手写填空区域。
                  </div>
                </div>
              </div>

              <div class="step-card">
                <div class="step-badge">步骤 3</div>
                <div class="step-body">
                  <div class="step-title">预览、微调、导出</div>
                  <div class="step-text">
                    参数改动约 300 毫秒后自动渲染，也可以点预览区下方的<strong>「↻ 刷新」</strong>立即重绘。
                    满意后点右栏底部的<strong>「导出图片」</strong>批量保存 PNG，或<strong>「导出 PDF」</strong>生成 300 DPI 打印级 PDF。
                    文字区域、笔迹角色都在右侧对应分组里，需要时再展开。
                  </div>
                </div>
              </div>

              <NAlert type="info" title="第一次使用的小提示" style="margin-top: 12px">
                <ul class="guide-list">
                  <li><strong>没有字体也能先用</strong>：首次启动会自动选系统楷体；想换字体在「② 字迹」里选。</li>
                  <li><strong>想先看效果</strong>：预览区空状态点「先看 30 秒示例」，会直接生成一张成品。</li>
                  <li><strong>不知道怎么走</strong>：右栏顶部有「上手三步」清单，完成字迹 / 内容 / 导出会自动打勾。</li>
                  <li><strong>随时重看引导</strong>：底部状态栏「✨ 新手引导」，或本窗口右下角按钮。</li>
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
                  然后在软件的<strong>「① 内容」</strong>组点击<strong>「导入 PDF / Word 打印底图」</strong>并选择该 PDF 文件。<br />
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

              <div class="feature-section">
                <div class="section-title">💡 贴心弹窗：不想要识别背景色 / 懒得改模板？</div>
                <div class="guide-text">
                  针对不同用户的使用习惯，软件会在导入时弹出确认选项：
                  <ul class="guide-list">
                    <li><strong>PDF 想要保留背景色（不擦除、不提取选框）</strong>：导入 PDF / Word 打印底图时选择<strong>「仅作为纯底图」</strong>，软件将完整保留文档原始色彩、图像与文字，原样作为背景信纸。</li>
                    <li><strong>Word 模板有高亮但想通篇全手写</strong>：导入 Word 正文文本 (.docx) 时选择<strong>「全文手写模式」</strong>，软件自动忽略所有高亮背景标记，无需手动改模板，整篇直接全手写排版。</li>
                  </ul>
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
                    <li><strong>取消选框（展示纯净预览）</strong>：<strong>在选框外侧的任意图纸空白处，或外侧灰色背景留白处单击鼠标</strong>（或按键盘 <code>Esc</code> 键），选框与手柄会立刻隐藏，恢复纯净清爽的原版预览。</li>
                    <li><strong>双击编辑属性</strong>：双击预览图上的区域或列表项，可打开详细对话框，修改文字、微调字号、单独覆盖字体或边距。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">2. 笔迹角色管理（多人书写与多字体混排）</div>
                <div class="guide-text">
                  在右栏「② 字迹」里点<strong>「多人笔迹 / 打印混排」</strong>展开角色列表：
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
                  在<strong>「③ 版式 → 高级：笔画扰动 / 写错字 / 边距」</strong>里找到<strong>「写错字」</strong>卡片（高级默认收起，点开即可）：
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
        <!-- Tab 5: 更多实用功能 -->
        <NTabPane name="more" tab="🧰 更多实用功能">
          <NScrollbar style="max-height: 520px" trigger="none">
            <div class="help-pane">
              <div class="feature-section">
                <div class="section-title">1. 预设系统：满意的参数一键复用</div>
                <div class="guide-text">
                  右栏顶部的<strong>「当前方案」</strong>下拉可切换内置方案；调出满意的排版与扰动参数后，点<strong>「存为预设」</strong>保存成文件，日后点<strong>「从文件载入」</strong>即可一键恢复整套配置。<NTag size="small" type="info" style="margin-top: 6px">💡 笔迹角色配置随预设一并保存，与 Python 版格式互通</NTag>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">2. 纯背景预览：先摆版式，再填内容</div>
                <div class="guide-text">
                  无需输入任何文字：选好纸张（或导入 PDF / Word 打印底图）后，左侧预览区会直接显示底图。先浏览整体版式、规划每处手写区域的位置，再逐页框选与录入文字，思路更顺畅。
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">3. 边距与边界提示</div>
                <div class="guide-text">
                  <ul class="guide-list">
                    <li><strong>边距</strong>：在「③ 版式 → 高级」里分别设置上 / 下 / 左 / 右留白，控制正文在纸张中的落笔范围。</li>
                    <li><strong>边界提示（仅预览）</strong>：勾选后预览图上会用彩色框直观标出实际渲染边界，导出成品不受影响——对齐横线纸张、排查文字溢出时特别好用。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">4. 个性化外观与舒适度</div>
                <div class="guide-text">
                  <ul class="guide-list">
                    <li><strong>文字颜色</strong>：支持 <code>#RRGGBB</code> 自定义墨水色，碳黑、蓝黑、红笔随意换。</li>
                    <li><strong>深色模式</strong>：底部状态栏一键轮转 🖥️ 跟随系统 / ☀️ 浅色 / 🌙 深色主题（也可在「关于」中精准选择）。</li>
                    <li><strong>预览底色</strong>：预览区下方可切换浅灰绿 / 深灰两档视口底色，长时间盯屏更护眼。</li>
                  </ul>
                </div>
              </div>

              <div class="feature-section">
                <div class="section-title">5. 应用内自动更新</div>
                <div class="guide-text">
                  软件启动时自动检查新版本（可在「关于」中关闭）；也可随时在<strong>「关于」→「检查更新」</strong>手动检查。更新包下载后自动完成覆盖升级并重启，无需手动解压替换。
                </div>
              </div>
            </div>
          </NScrollbar>
        </NTabPane>
      </NTabs>
    </div>

    <template #footer>
      <div style="display: flex; justify-content: space-between; align-items: center">
        <NButton quaternary size="small" @click="restartOnboarding()">
          ✨ 重新观看新手引导
        </NButton>
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
