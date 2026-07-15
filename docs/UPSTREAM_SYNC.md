# 上游 Grok Build Core 同步指南

> 状态：**已确认**（与 `DESKTOP_DEV.md` ADR-6 / ADR-8 一致）  
> 日期：2026-07-16

本文约定如何把 **上游 Grok Build 非 UI 核心** 持续合入本仓（Grok Desktop monorepo），同时不让 Desktop UI / Tauri 与上游 TUI 缠在一起。

---

## 1. 目标

| 目标 | 说明 |
|------|------|
| 可持续合入 | 上游 shell / tools / sampler / workspace 等修复与功能能进入本产品 |
| 一体交付 | Desktop 安装包始终带 **绑定版本** 的 core，不依赖用户 PATH 上的 CLI |
| 边界清晰 | Desktop 专用代码不污染 core；合入冲突可预期 |

**明确不同步为产品目标：** 上游 TUI（`xai-grok-pager` 的 ratatui 界面体验）。Desktop 运行时 **不依赖** pager UI。

---

## 2. 已确认方案

| 项 | 决策 |
|----|------|
| 同步机制 | **`git remote` + 定期 `merge`**（紧急用 `cherry-pick`） |
| 不用 | git submodule、周期 vendor 整树拷贝（作主路径） |
| 备选 | 仅当未来拆成「瘦壳仓」时再评估 **git subtree** |
| 通信（运行时） | 内置 agent **子进程 + stdio ACP**（与上游 agent 入口对齐，降低合入后适配成本） |

---

## 3. 路径分类

| 类别 | 路径（示例） | 合入策略 |
|------|----------------|----------|
| **A. Core（主战场）** | `crates/codegen/xai-grok-shell`, `xai-grok-sampler`, `xai-grok-tools`, `xai-grok-workspace`, `xai-acp-lib`, `xai-chat-state`, `xai-grok-config*`, `xai-grok-models`, `xai-grok-mcp`, `xai-grok-auth`, … 及 core 依赖的 `crates/common/**` | **优先取上游**；本地仅保留已记录的必要 fork |
| **B. TUI-only** | `crates/codegen/xai-grok-pager` 视图与 ratatui 资源；pager 专用 playground | Desktop **不依赖**；冲突可快速偏向 upstream 或 ours，避免为 TUI 耗时间 |
| **C. Desktop-only** | `apps/desktop/**`, `packages/**`, `docs/DESKTOP_DEV.md`, `docs/UPSTREAM_SYNC.md` | **永不来自上游**；禁止把业务塞进 A 类 crate |
| **D. 薄适配** | Tauri host 内 spawn/stdio bridge、资源路径、打包 embedding | 允许本仓演进；保持薄，不实现 session/tool 业务 |
| **生成物** | 根 `Cargo.toml`（若为生成）、lockfile | 按仓约定处理；merge 后重新 check / 必要时 regenerate |

---

## 4. Remote 配置

```bash
# 在本仓根目录（一次性）
git remote add upstream <UPSTREAM_GROK_BUILD_URL>

# 例（占位——有官方/镜像地址后替换并更新本文）
# git remote add upstream https://github.com/example/grok-build.git

git fetch upstream
```

| 字段 | 值 |
|------|-----|
| **upstream URL** | _TODO：填入实际上游 Grok Build 仓库 URL_ |
| **默认同步分支** | `main`（或上游默认分支；以 remote 为准） |
| **建议节奏** | 跟上游 **release** 合一次；日常可 **每周** fetch+merge；安全/严重回归 **随时 cherry-pick** |

记录每次同步：

| 日期 | 上游 ref / tag | 操作人 | 备注 |
|------|----------------|--------|------|
| _YYYY-MM-DD_ | _tag 或 sha_ | _name_ | 首次配置 / 冲突要点 |

---

## 5. 标准 merge 流程

```bash
git fetch upstream
git checkout main   # 或你的集成分支
git merge upstream/main
# 解决冲突（见 §6）
# 重新生成/校验 workspace 若需要

# 最低验证门禁
cargo check -p xai-grok-shell
cargo check -p xai-grok-sampler
# Desktop 可用后：
#   构建 apps/desktop + ACP smoke（内置 agent stdio 拉起一轮会话）
```

**紧急热修：**

```bash
git fetch upstream
git cherry-pick <upstream-commit-sha>
# 跑同样门禁
```

**禁止：** 把上游整树用「删了重拷」覆盖本仓（会毁掉 Desktop 历史与可追溯 merge）。

---

## 6. 冲突处理规则

| 冲突位置 | 规则 |
|----------|------|
| A 类 core 行为 / API | **跟上游**；再改 `apps/desktop` 或薄 host 适配 |
| A 类仅格式/无关 churn | 尽量避免本仓主动制造；冲突时取 upstream |
| B 类 pager TUI | 不阻塞发布；可选 `ours`/`theirs` 快速了结 |
| C 类 Desktop | 只保留本仓版本 |
| 需要 Desktop 专用 core 行为 | **不要**长期 `#ifdef`；优先通用能力 PR 回上游；过渡期在 **D 适配层** 包装，并记入 §8 fork 清单 |

---

## 7. 本仓改 core 的纪律

1. **能不改 core 就不改** — UI、托盘、Provider 向导、快捷键放 Desktop。  
2. **必须改 core** — 设计成上游也能用的 API（如更稳的 agent ready 信号、`models --json`），并争取回馈上游。  
3. **临时 fork** — 必须写入 §8，写明原因与「可删除条件」。  
4. **禁止** 在 `xai-grok-shell` 等 crate 依赖 Tauri / 前端。  

运行时对齐：Desktop 默认 **子进程 + stdio ACP**，与上游 `grok agent stdio` 同模型，减少「只为 Desktop 存在的嵌入路径」。

---

## 8. 已知 fork / 补丁清单

> merge 前必看；merge 后更新状态。

| ID | 路径 / 说明 | 原因 | 状态 | 可删除条件 |
|----|-------------|------|------|------------|
| — | _暂无_ | | | |

---

## 9. 跳过 / 低优先级路径（merge 时少纠缠）

- `crates/codegen/xai-grok-pager` 的 TUI 视图、主题 playground、终端专用渲染细节  
- 仅服务 TUI 的文档截图与 keybinding 文案（Desktop 自有文档）  
- 上游 CI/Bazel 若与本仓无关且冲突剧烈 — 以本仓 Desktop CI 为准，不强制对齐全套上游 pipeline  

---

## 10. CI 门禁（目标）

同步合并后至少：

1. 关键 core crate `cargo check` / 既有测试子集  
2. Desktop 构建（Tauri）  
3. **ACP smoke**：内置 agent stdio → initialize → session → 一轮 prompt  

未通过不得标为「已同步可发版」。

---

## 11. 相关文档

- [DESKTOP_DEV.md](./DESKTOP_DEV.md) — 产品架构、ADR-1/6/7/8、技术栈  
- 仓内 user-guide（pager）— 行为参考；Desktop UI 不照搬实现  

---

## 12. 变更记录

| 日期 | 变更 |
|------|------|
| 2026-07-16 | 初版；确认 remote merge + 子进程 stdio 策略 |
