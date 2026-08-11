# 许可策略

## 现状

- **应用代码**：MIT（`LICENSE`）
- **Slint**：crates.io 发布版默认 **GPLv3**

## 为什么 crates.io 默认是 GPLv3

crates.io 上 `slint` crate 的默认许可是 GPLv3——这是发布平台的默认选择。
GPLv3 免费使用，条件是程序整体遵循 GPLv3（本项目本就是开源项目，无额外负担）。

## 当前选择：GPLv3 + MIT 开源

本项目**确定开源**（MIT），因此：
- 直接使用 crates.io 的 GPLv3 版 Slint，程序整体按 GPLv3 分发
- 应用自身代码仍标注 MIT（GPL 传染作用于"完整程序分发"，源码模块保留各自许可）
- GPLv3 无归因展示义务，GUI 不显示 `AboutSlint`

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

- [ ] 发布包不含字体文件（版权隔离，与 Python 版一致）