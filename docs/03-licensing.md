# 许可策略

## 现状

- **应用代码**：MIT（`LICENSE`）
- **Slint**：crates.io 发布版默认 **GPLv3**

## 为什么 crates.io 默认是 GPLv3

Slint 提供三种许可，按需选择（官方 README / Licensing FAQ）：

| 许可 | 费用 | 场景 | 条件 |
|------|------|------|------|
| **GPLv3** | 免费 | 开源应用（含嵌入式） | 程序整体遵循 GPLv3 |
| **Royalty-free** | 免费 | **闭源商用**桌面/移动/Web | 应用内显示 AboutSlint 或下载页放徽章；禁止嵌入式 |
| **商业付费** | 付费 | 嵌入式、大企业 | 无限制 |

crates.io 上 `slint` crate 的默认许可是 GPLv3——这是发布平台的默认选择，
不表示只能用 GPLv3。

## 当前选择：GPLv3 + MIT 开源

本项目**确定开源**（MIT），因此：
- 直接使用 crates.io 的 GPLv3 版 Slint，程序整体按 GPLv3 分发
- 应用自身代码仍标注 MIT（GPL 传染作用于"完整程序分发"，源码模块保留各自许可）
- GUI 内置 `AboutSlint` 组件（`main_window.slint` 底部），为未来切换预埋

## 未来商业化切换路径

若未来出现闭源商业化需求（桌面/移动/Web 应用）：

1. **切换 Slint 依赖为 git 源**（源码仓库包含 Royalty-free 许可文件）：

   ```toml
   [dependencies]
   slint = { git = "https://github.com/slint-ui/slint", tag = "v1.x.x" }
   ```

2. **保持 `AboutSlint` 显示**（已预埋）满足归因义务。
3. 应用代码仍可保持 MIT（或按需调整）。

⚠️ 嵌入式场景（把软件装进设备/工控屏销售）不受 Royalty-free 覆盖，
需购买商业许可。

## 其他依赖许可

| crate | 许可 | 备注 |
|-------|------|------|
| ab_glyph | MIT/Apache-2.0 | 字体光栅化 |
| image | MIT/Apache-2.0 | 图像 IO |
| rand / rand_distr | MIT/Apache-2.0 | 随机数 |
| serde / serde_json | MIT/Apache-2.0 | 序列化 |
| thiserror | MIT/Apache-2.0 | 错误类型 |
| tempfile | MIT/Apache-2.0 | 测试 |

全部宽松许可，无商业约束。

## 合规检查

- [ ] GUI 底部可见 `AboutSlint`（切换 Royalty-free 后仍满足）
- [ ] 发布包不含字体文件（版权隔离，与 Python 版一致）
- [ ] 若未来切换许可，更新本文件与 README 的许可说明