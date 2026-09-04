# 发布规范（Release Process）

本文是发版的唯一规范。每次发版严格按此执行，避免漏改版本号、更新介绍缺失、CI 发版失败。

## 1. 版本号：5 处必须一致

发版前把以下 5 处同步为新版本号（不带 `v` 前缀，如 `0.4.0`）：

| # | 文件 | 字段 |
|---|------|------|
| 1 | `package.json` | `"version"` |
| 2 | `src/store.ts` | `APP_VERSION`（软件内显示的当前版本，也是检查更新的比对基准） |
| 3 | `src-tauri/tauri.conf.json` | `"version"` |
| 4 | `src-tauri/Cargo.toml` | `version` |
| 5 | `crates/core/Cargo.toml` | `version` |

> 注意：`Cargo.lock` 不要手改，改完上面 5 处后跑一次 `cargo check --workspace` 即自动同步；
> 否则 CI 的 `cargo nextest run --workspace --locked` 会因 lock 过期直接失败。
> `APP_VERSION` 漏改是最常见的坑——软件内“当前版本”会显示旧号，且可能对新版误报“发现新版本”。

验证命令：

```powershell
Select-String '"version": "0.4.0"' package.json, src-tauri/tauri.conf.json
Select-String 'APP_VERSION = "0.4.0"' src/store.ts
Select-String '^version = "0.4.0"' src-tauri/Cargo.toml, crates/core/Cargo.toml
```

## 2. 更新介绍：只写 CHANGELOG，不手写 Release

Release 说明由 CI 自动生成，**不要在 GitHub 网页上手动编辑 Release 正文**（下次打 tag 会被覆盖，且软件内读的是 Release 正文，手动改了也容易与 CHANGELOG 脱节）。

### 2.1 CHANGELOG 格式（强约束，CI 依赖）

- 文件：`CHANGELOG.md`，遵循 Keep a Changelog + SemVer。
- 开发期间记在 `## [Unreleased]` 下；发版时整体搬到新小节：
  `## [0.4.0] - 2026-09-04`，上面留空的 `## [Unreleased]`。
- 小节标题必须严格匹配正则 `^## \[x.y.z\]`，否则 `.github/release-notes.sh` 找不到小节，`release` job 直接失败：
  `CHANGELOG.md 中未找到版本 [x.y.z] 小节`。
- 小节内用 `### Added / Fixed / Changed` 分类。

### 2.2 Release 正文结构（CI 生成）

`.github/release-notes.sh` 逻辑：

1. 用 awk 提取 `## [x.y.z]` 到下一个 `## ` 之间的全部内容；
2. 前面加上 `## 更新内容`；
3. 后面拼上 `packaging/release-body.md`（产物下载直达链接 + 说明，`{TAG}` 占位符自动替换为实际 tag）。

最终 Release 正文 = `## 更新内容`（CHANGELOG 本版小节） + `## 产物下载` + `## 说明`。

### 2.3 软件内“更新介绍”显示规则（已验证，可直接在软件里看）

- 入口：启动时自动检查（可关）+ 「关于」→「检查更新」→ 发现新版弹窗（`src/components/UpdateModal.vue`）。
- 数据链：`check_for_updates(APP_VERSION)` → `src-tauri/src/updater.rs` 三级容灾
  （GitHub REST API → Atom 订阅源 + expanded_assets → 网页 302 兜底）取最新 Release，
  经 `UpdateInfo.body` 传给前端 `<pre>{{ info.body }}</pre>` 展示。
- 裁剪规则（`trim_release_notes_markdown` / `html_release_body_to_text`）：**只保留第一个 `## ` 小节**，
  即 `## 更新内容` 部分；第二个 `## `（`## 产物下载`）起的固定样板在软件内会被截掉。
- 结论：
  - 用户在软件内看到的“更新内容” = Release 正文的第一个 `## ` 小节 = CHANGELOG 本版小节。**是的，可直接在软件里看，无需翻 GitHub。**
  - 重要更新说明必须写进 CHANGELOG 本版小节；写进 `release-body.md` 模板或第二个 `## ` 之后的内容，软件内看不到。

## 3. 发版步骤（checklist）

1. 确认 `master` 在目标提交上，工作区干净（`git status --short` 无输出）。
2. 同步 5 处版本号（见 §1）并验证。
3. 整理 `CHANGELOG.md`：`[Unreleased]` → 新版本小节 + 日期，补齐本次新增条目，上面保留空 `[Unreleased]`。
4. 本地验证：
   - `cargo test -p handwrite-sim --lib` 通过；
   - `cargo clippy -p handwrite-sim --lib -- -D warnings` 通过；
   - 预览 release notes 提取：确认 `## [x.y.z]` 小节存在且非空。
5. Commit（只含版本 + CHANGELOG，不混功能改动），push 到 `origin/master`。
6. 确认 CI `master` 分支构建为绿色后，打 tag 并推送：
   `git tag v0.4.0; git push origin v0.4.0`（tag 必须 `v*` 格式，否则 `release` job 不触发）。
7. 等 CI `release` job 完成，检查：
   - GitHub Releases 页面出现 `v0.4.0`，正文以 `## 更新内容` 开头；
   - 资产齐全：3 平台 zip + 3 平台单文件 + Windows webview2 包（共 7 个文件）。
8. 软件内验证：用旧版点「检查更新」，确认弹窗版本号正确且“更新内容”与 CHANGELOG 一致。

## 4. 常见坑

| 坑 | 后果 | 对策 |
|----|------|------|
| 只改了 Cargo 版本，漏了 `APP_VERSION` | 软件内版本号不变，更新判断错乱 | 按 §1 逐项 grep 验证 |
| CHANGELOG 只有 `[Unreleased]`，没建 `[0.4.0]` 小节 | `release` job 报错退出，无 Release | 打 tag 前确认小节存在 |
| 标题写成 `## 0.4.0`（缺方括号） | awk 匹配不到，同上 | 严格用 `## [0.4.0] - YYYY-MM-DD` |
| 重要说明写在第二个 `## ` 之后 | 软件内更新弹窗看不到 | 重要内容一律进 CHANGELOG 小节 |
| 手动在网页改 Release 正文 | 与 CHANGELOG 脱节，下次被覆盖 | 只改 CHANGELOG + 模板，重打 tag 修正 |
| master 推送与 tag 推送间隔太短 | concurrency 取消旧运行（预期内） | 先等 master 变绿再打 tag |
