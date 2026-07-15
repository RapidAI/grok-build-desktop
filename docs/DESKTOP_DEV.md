# Grok Desktop — 开发文档

> 状态：架构决策已锁定（可进入 M1）  
> 日期：2026-07-16  
> 产品：**Grok Desktop** — 一体安装、一体运行的桌面程序（非“外挂已装 CLI”）  
> 内核：借鉴 / 内嵌 **Grok Build 非 UI 核心**；UI 为 Desktop 自有（Tauri + React）  
> **已决汇总：**  
> - 壳 = **Tauri 2 + React + TypeScript**  
> - 进程/传输 = **内置 agent 子进程 + ACP over stdio**（WS 可选；默认非 in-process）  
> - 上游同步 = **git remote + 定期 merge**（见 [`UPSTREAM_SYNC.md`](./UPSTREAM_SYNC.md)）

---

## 1. 目标与非目标

### 1.1 产品定位（已确认）

| 维度 | 说明 |
|------|------|
| **一体程序** | 用户安装 **一个** Grok Desktop；内含 UI + agent 内核。不依赖用户事先安装 `grok` CLI |
| **代码策略** | **借鉴 / 复用** 本仓（及上游）Grok Build 代码，而不是从零写 agent |
| **上游合并** | 长期能 **合并上游 Grok 核心（非 UI）改动**；Desktop UI 与上游 TUI 解耦，冲突可控 |
| **产品形态** | 多 Agent 指挥中心布局，对标 Codex Desktop |
| **平台** | Windows、macOS、Linux 一等公民 |
| **模型** | 原生 Grok + **第三方**（OpenAI **Responses** 与 **Chat Completions** 必选；Messages 内核已有） |
| **数据兼容** | 配置 / 会话 / 鉴权与 Grok Build 约定对齐（如 `~/.grok`），便于与 CLI 生态互通；**不以“外置 CLI”为运行前提** |

### 1.2 非目标（首期）

- 不重写 `xai-grok-shell` / tools / workspace / sampler 业务内核  
- 不移植 ratatui TUI 为桌面 UI；**上游 `xai-grok-pager`（TUI）不是 Desktop 依赖**  
- 不做“薄壳调用用户 PATH 上的 grok”作为正式产品形态（开发期可临时用）  
- 不在 Desktop 专用 fork 里大改核心业务，导致无法合上游  
- 不采用 Electron（见 ADR-2）

---

## 2. 现有代码库 Review

### 2.1 仓库定位

本仓库是 **Grok Build（`grok`）** 的 Rust 源码树：全屏 TUI 编码 Agent，同时支持 headless、以及通过 **ACP（Agent Client Protocol）** 嵌入编辑器/其它客户端。

```
composition root:  crates/codegen/xai-grok-pager-bin
TUI 层:            crates/codegen/xai-grok-pager        (ratatui 渲染、输入、slash、dashboard)
Agent 内核:        crates/codegen/xai-grok-shell        (session、auth、leader、extensions)
采样/推理:         crates/codegen/xai-grok-sampler      (HTTP 流式 + 多协议后端)
工具:              crates/codegen/xai-grok-tools
工作区:            crates/codegen/xai-grok-workspace
协议传输:          crates/codegen/xai-acp-lib
配置/模型目录:     xai-grok-config*, xai-grok-models
```

> 根 `Cargo.toml` 为生成物，按仓内约定以各 crate 的 `Cargo.toml` 为准。

### 2.2 分层架构（现状）

```text
┌──────────────────────────────────────────────────────────────────┐
│  Clients                                                         │
│  · TUI (xai-grok-pager)                                          │
│  · Headless (grok -p)                                            │
│  · ACP stdio (grok agent stdio)                                  │
│  · WS serve (grok agent serve)                                   │
│  · Leader multi-client (UDS IPC + ACP 转发)                      │
└────────────────────────────┬─────────────────────────────────────┘
                             │ ACP JSON-RPC / Leader framed IPC
┌────────────────────────────▼─────────────────────────────────────┐
│  Agent Runtime (xai-grok-shell)                                  │
│  MvpAgent · SessionActor · ChatState · Tools · MCP · Memory …    │
└────────────────────────────┬─────────────────────────────────────┘
                             │ SamplerHandle / SamplingEvent
┌────────────────────────────▼─────────────────────────────────────┐
│  Sampler (xai-grok-sampler)                                      │
│  ApiBackend: chat_completions | responses | messages             │
└────────────────────────────┬─────────────────────────────────────┘
                             │ HTTPS
                    SpaceXAI / OpenAI-compat / Anthropic / Ollama …
```

**关键结论：内核已经按「多客户端」设计。** Desktop 应成为又一个 ACP 客户端，而不是 fork 一套 agent。

### 2.3 内核能力清单（Desktop 应直接复用）

| 能力 | 位置 / 入口 | Desktop 含义 |
|------|-------------|--------------|
| 会话生命周期 | ACP `session/new`、load、prompt、update | 多线程会话列表 + 流式对话 |
| 权限 / 提问 | permission + `ask_user_question` | 桌面模态框 / 侧栏审批 |
| 工具流 | 文件读写、edit、bash、web、image/video… | 富渲染：diff、终端、图表 |
| Subagent / Task | spawn、capability modes、worktree isolation | 多 Agent 并行看板 |
| Plan mode | enter/exit plan | 计划审阅 UI |
| Dashboard | TUI `/dashboard`、`grok dashboard` | Desktop 首页的原型（多 agent 状态机已存在） |
| MCP / Skills / Plugins / Hooks | shell + tools + config | 设置页管理，逻辑在内核 |
| Sandbox / Permissions | workspace + sandbox crates | 桌面展示策略与审批，不重写沙箱 |
| Auth | OAuth/OIDC、device code、API key、`~/.grok/auth.json` | 系统浏览器登录 + 密钥保险箱 |
| 会话落盘 | `~/.grok/sessions/<cwd>/<id>/` | 与 CLI 共享 resume |
| Leader | 单机多客户端共享一个 agent 进程 | Desktop + CLI 可共享同一内核进程 |

### 2.4 模型与推理子系统（重点）

#### 2.4.1 现状：三协议已落地

`xai-grok-sampler` 统一抽象采样层，按 `ApiBackend` 分发：

| `api_backend` | 协议 | 典型路径 |
|---------------|------|----------|
| `chat_completions`（默认） | OpenAI Chat Completions | `{base_url}/chat/completions` |
| `responses` | OpenAI Responses | `{base_url}/responses` |
| `messages` | Anthropic Messages | `{base_url}/messages` |

流式管线：`SamplingClient` → `stream_chat_completions` / `stream_responses` / `stream_messages` → `SamplingEvent`。

#### 2.4.2 配置面（`~/.grok/config.toml`）

内核已支持用户级自定义模型（文档：`11-custom-models.md`），例如：

```toml
[models]
default = "grok-build"

# 原生 / 默认目录（prefetch + hardcoded）
# 也可覆盖内置字段

[model.gpt-4o]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o"
env_key = "OPENAI_API_KEY"
# api_backend 默认 chat_completions

[model.gpt-4o-responses]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o (Responses)"
api_backend = "responses"
env_key = "OPENAI_API_KEY"

[model.ollama-local]
model = "codellama"
base_url = "http://localhost:11434/v1"
name = "CodeLlama (Ollama)"
```

凭证解析顺序：`api_key` → `env_key`（可数组）→ 登录会话 token → 全局 `XAI_API_KEY`。

另有：`extra_headers`、`temperature`、`context_window`、`GROK_MODELS_BASE_URL` 拉取远端 `/models` 目录等。

#### 2.4.3 Desktop 对模型的增量需求

内核能力已够用；Desktop 要补的是 **产品化与 UX**：

1. **可视化 Provider 管理**（添加 / 编辑 / 测试连接 / 删除），不要求用户手写 TOML  
2. **协议选择器**：明确标注 Chat Completions vs Responses（及 Messages）  
3. **密钥安全存储**：优先 OS keychain / 凭据管理器，写入配置时避免明文落盘（可写 `env_key` 或加密引用）  
4. **连接诊断**：base URL 可达性、401/404、协议不匹配提示、流式是否正常  
5. **模型目录同步**：对 OpenAI-compat 的 `GET /v1/models` 做列表缓存与手动刷新  
6. **会话级模型切换**：与现有 `/model`、session meta 一致，Desktop UI 与 CLI 互见  
7. **能力差异提示**：部分工具/reasoning/多模态依赖后端能力；非 Grok 后端时降级展示或禁用不可用能力  
8. **BYOK 与原生账号并存**：同一应用内可同时有 SpaceXAI 登录态 + 多个第三方 Provider

**结论：第三方模型不是“新协议栈”，而是把已有 sampler + config 模型系统做成一等公民 UI。**

### 2.5 对 Desktop 最友好的集成入口

| 模式 | 命令 | 评价 |
|------|------|------|
| **ACP stdio** | `grok agent stdio` | 与 IDE 集成一致；进程模型清晰；推荐作为 Desktop 主路径之一 |
| **ACP WebSocket serve** | `grok agent serve --bind 127.0.0.1:…` | 多窗口 / 热重载调试友好；适合 Tauri 长连接 |
| **Leader** | `grok agent leader` + 客户端 attach | 与 TUI/IDE 共享同一 agent；适合“本机唯一内核” |
| **In-process spawn** | pager 内 `spawn_grok_shell` | 当前 TUI 路径；Tauri 宿主可链库，但进程隔离更弱；**默认不用** |
| Headless `-p` | 单轮脚本 | 不适合交互式 Desktop |

**推荐主路径（与 Codex 同构）：Desktop 启动或附着本地 agent（stdio 或 loopback WS），全部会话走 ACP。**

### 2.6 风险与缺口（Review 发现）

| 项 | 说明 | 影响 |
|----|------|------|
| Windows 官方构建 | README 写明 Windows 为 best-effort | Desktop 必须把 Win CI / 打包提为一等优先级 |
| Leader 传输 | 文档与实现偏 Unix domain socket | Windows 需确认 named pipe / 等价路径，否则 Desktop 用 stdio/WS 规避 |
| ACP 扩展方法 | 大量 `x.ai/*` meta 与扩展 RPC | Desktop 协议客户端需版本协商与能力探测 |
| TUI 专用渲染 | mermaid、diff、scrollback 强依赖终端 | Desktop 需 Web/GPU 侧重新实现展示层 |
| 权限 UX | 终端数字键 / 快捷键审批 | Desktop 要图形化审批与默认策略配置 |
| 模型能力矩阵 | 工具调用、thinking、图片入站在不同 provider 表现不一 | 需兼容层与 UI 降级策略 |
| 自动更新 | CLI 有 `grok update` | Desktop 需独立更新通道（Sparkle / Squirrel / 自建） |
| 根 workspace 体量大 | 全量 `cargo build` 很重 | Desktop 工程应只依赖 shell/sampler 闭包，避免拖入 pager TUI |

---

## 3. Codex Desktop 对标

### 3.1 产品布局（期望对齐）

Codex Desktop 定位是 **Agent 指挥中心**，而非单聊窗口。Grok Build Desktop 建议布局：

```text
┌────────────┬───────────────────────────────────┬──────────────────┐
│ 侧栏       │ 主区                               │ 右栏（可折叠）   │
│            │                                   │                  │
│ · 工作区   │  会话流式对话                      │ · 文件 diff      │
│ · Agent 列 │  工具卡片 / 思考块 / 计划          │ · 终端输出       │
│ · Inbox    │  底部 Composer（@ 文件、模型选择） │ · TODO / Goal    │
│ · 设置入口 │                                   │ · 权限请求       │
└────────────┴───────────────────────────────────┴──────────────────┘
 顶部：工作区路径 · 模型 · 权限模式 · 账号 / Provider
```

与现有 TUI **Dashboard**（多 session 状态、peek、dispatch）概念对齐，Desktop 将其升级为默认首页。

### 3.2 Codex 架构要点（可借鉴）

公开逆向/官方信息显示 Codex Desktop 核心模式是：

1. **CLI-as-Backend** — Electron 不重写 agent，而是启动 `codex app-server`  
2. **三进程** — Renderer（React） / Main（Node IPC） / Rust CLI  
3. **职责分离** — UI 无业务逻辑；鉴权与 git 等在 main；智能在 Rust  
4. **双存储** — UI 状态与对话历史分库，避免跨进程锁  

Grok 侧已有等价物：`grok agent stdio|serve|leader` + `~/.grok/sessions`。

### 3.3 Grok 相对 Codex 的差异优势

- 已开源/可见的完整 agent crate 边界清晰  
- 原生 Dashboard、Goal、Subagent、Plan、Scheduler/Monitor 已在内核  
- **自定义模型与三协议 sampler 已成熟**，Desktop 可直接产品化 BYOK  
- ACP 标准客户端生态可复用  

---

## 4. 技术栈选型（已决）

### 4.1 决策

| 项 | 选择 |
|----|------|
| **桌面壳** | **Tauri 2** |
| **前端** | **React + TypeScript + Vite** |
| **Agent 内核** | **本仓 Grok Build core crates**（与安装包一体编译 / 捆绑） |
| **UI↔内核边界** | **ACP**（进程内通道或一体包内子进程；对用户仍是单一应用） |
| **否决** | Electron；依赖用户 PATH 上外置 `grok` 作为正式形态 |

**选定理由（摘要）：**

1. **一体产品**：安装包自带内核，版本锁定，行为可复现。  
2. **与核心同仓同语言**：便于复用 crate、合并上游非 UI 改动。  
3. **包体 / 内存**相对 Electron 更合适常驻多 Agent。  
4. **系统集成**（托盘、密钥、对话框）自然。  
5. React 做 Codex 级 UI；推理仍在 core。

**已知代价（接受）：** 系统 WebView 差异；PTY 需插件/自研；合并上游时需守住分层（见 §5.0）。

### 4.2 技术明细

| 层 | 技术 | 职责 |
|----|------|------|
| **产品壳** | **Tauri 2** | 窗口、托盘、通知、密钥、更新、**一体进程生命周期** |
| **UI** | React 18+、TypeScript、Vite | 指挥中心布局、设置、Provider 管理 |
| **样式** | Tailwind 或 CSS Modules + 设计 token | 暗色优先 |
| **对话渲染** | remark/rehype + Shiki + Mermaid | 替代 TUI scrollback |
| **终端** | xterm.js + PTY 插件（可后期） | 工具/终端输出 |
| **状态** | Zustand/Jotai + TanStack Query | 仅 UI；会话真相在 core |
| **协议** | `packages/acp-client`（TS）+ Rust bridge | ACP；便于测与以后多前端 |
| **Core（上游可合）** | `xai-grok-shell`、sampler、tools、workspace… | **唯一**推理与工具路径 |
| **Desktop-only Rust** | `apps/desktop/src-tauri`、可选 `xai-grok-desktop-host` | 窗口与 bridge；**禁止塞进上游 core 业务** |
| **配置 / 会话** | 与 Grok Build 同约定（`~/.grok` 等） | 兼容生态；一体应用内完成读写 |
| **打包** | 单一 Desktop 安装包（内含 core） | 不强制用户装 CLI |
| **CI** | 三平台构建 + **定期 merge 上游 core 演练** | 签名 / 公证 |

### 4.3 明确不推荐 / 不做

- 改用 Electron  
- 在 TypeScript 重写 agent loop  
- Desktop 直连模型 API 绕过 sampler  
- 为 Desktop 方便而大改 core 公共 API 却不回馈/不对齐上游  
- 把 Tauri/React 依赖写进 `xai-grok-shell` 等上游核心 crate  
- 仅支持单一模型协议  

---

## 5. 目标架构

### 5.0 一体应用 + 上游可合并分层（核心约束）

```text
┌─────────────────────────── Grok Desktop（一个安装包 / 一个产品）───────────────────────────┐
│                                                                                            │
│  ┌─ Desktop 自有（不与上游 TUI 合并）──────────────────────────────────────────────────┐  │
│  │  React UI · Tauri host · desktop 主题/快捷键 · Provider 向导 UI · 托盘/更新          │  │
│  └───────────────────────────────────────────┬──────────────────────────────────────────┘  │
│                                              │ ACP（逻辑边界，用户无感）                    │
│  ┌─ Grok Build Core（对齐 / 合并上游非 UI）──▼──────────────────────────────────────────┐  │
│  │  shell · sampler · tools · workspace · config · auth · mcp · memory · acp-lib …      │  │
│  │  （可含 agent 入口二进制，由 Desktop 内置启动，而非依赖系统 PATH）                      │  │
│  └──────────────────────────────────────────────────────────────────────────────────────┘  │
│                                                                                            │
│  不进入 Desktop 运行时依赖：xai-grok-pager 的 ratatui TUI 视图（上游可继续演进，Desktop 不跟）│
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

**原则：**

| 原则 | 做法 |
|------|------|
| **一体交付** | 用户只装 Desktop；core 版本与 UI 版本在构建时绑定 |
| **借鉴 core，不 fork 业务** | core crates 保持与上游可 diff / merge；Desktop 差异放在 `apps/desktop` 与薄 adapter |
| **UI 双轨** | 上游 TUI（pager）与 Desktop UI **并行**；共享 core，不共享渲染 |
| **边界稳定** | UI↔core 经 **ACP + 稳定配置文件**；少改 core 的“桌面专用特化” |
| **上游合入** | 只合 **非 UI**（shell/tools/sampler/…）；冲突优先解决在 adapter，不把 Desktop 逻辑打进 core |

**crate 分类（合并策略）：**

| 类别 | 示例 | 合并上游 |
|------|------|----------|
| **A. Core（应频繁合）** | `xai-grok-shell`, `xai-grok-sampler`, `xai-grok-tools`, `xai-grok-workspace`, `xai-acp-lib`, `xai-chat-state`, `xai-grok-config*`, `xai-grok-models`, `xai-grok-mcp`, … | **是** — 尽量少本地补丁 |
| **B. TUI-only（可不跟）** | `xai-grok-pager` 视图/ratatui、pager 专用 assets | **否** — Desktop 不依赖；可选同步 headless/CLI 入口若需要 |
| **C. Desktop-only（永不与上游抢）** | `apps/desktop/**`, `packages/acp-client`, desktop 文档/品牌 | 仅本产品演进 |
| **D. 薄适配（尽量薄）** | `xai-grok-desktop-host`：spawn/channel、资源路径、打包 embedding | 允许小改；**禁止**塞 session/tool 业务 |

### 5.1 进程模型（一体程序内部）

对用户是 **一个 app**；进程内是否再拆 agent 子进程是实现细节。

```text
推荐（隔离好、易合上游 agent 入口）：
  Grok Desktop.exe / .app
    ├─ Tauri + WebView (UI)
    └─ 内置 agent 子进程（同安装目录二进制或 side-by-side）
           └─ xai-grok-shell …  经 stdio 或 loopback WS 讲 ACP

可选（更“单进程”，合上游时注意 runtime 嵌入）：
  Grok Desktop
    └─ Tauri main thread/runtime + in-process MvpAgent（仍走 ACP 消息形态）
```

**一体程序 ≠ 必须单 OS 进程**；一体 = **统一安装、统一版本、统一入口、用户无感**。  
默认更倾向 **内置子进程 + ACP**：agent panic 不拖死整个窗口，且与上游 `agent stdio/serve` 入口对齐，合并成本低。

### 5.2 协议边界

- **会话与 agent 控制**：ACP（`initialize`、`session/*`、permission、扩展 meta）  
- **Desktop 专属 UI 命令**（可选第二通道）：如 `desktop/models/list`、`desktop/models/upsert`、`desktop/models/test`——**优先实现为对 config 的受控写入 + 调用现有 models 列举 API**，避免平行配置源  
- **能力协商**：`initialize` 交换 client/server capabilities；旧内核新 UI 需优雅降级  

### 5.3 配置与数据源（单一真相）

| 数据 | 所有者 | 路径 / 来源 |
|------|--------|-------------|
| 用户配置 | 内核 / 共享文件 | `~/.grok/config.toml` |
| 模型目录 | 内核 | 内置 + prefetch + `[model.*]` |
| 鉴权 | 内核 AuthManager | `~/.grok/auth.json` + keychain |
| 会话 | 内核 | `~/.grok/sessions/...` |
| UI 偏好 | Desktop only | 如 `~/.grok/desktop/ui.json`（主题、侧栏宽度、上次工作区） |
| 自动任务 Inbox | Desktop 可先本地 SQLite | 与 Codex automations 类似；调度执行仍调内核 session |

### 5.4 第三方模型架构（详细）

```text
                    ┌─────────────────────┐
                    │  Settings → Models  │
                    │  Provider Wizard    │
                    └──────────┬──────────┘
                               │ upsert [model.<id>]
                               ▼
                    ┌─────────────────────┐
                    │  config.toml        │
                    │  api_backend = …    │
                    │  base_url / keys    │
                    └──────────┬──────────┘
                               │ reload / session model switch
                               ▼
┌──────────────┐    ┌─────────────────────┐    ┌──────────────────────┐
│ 原生 Grok    │    │  Sampler            │    │  第三方 Endpoint      │
│ SpaceXAI auth│───▶│  chat_completions   │───▶│  OpenAI / 代理 / 本地 │
│              │    │  responses          │    │  Ollama / vLLM / …    │
│              │    │  messages           │    │  Anthropic 等         │
└──────────────┘    └─────────────────────┘    └──────────────────────┘
```

#### 5.4.1 Provider 抽象（Desktop UI 模型）

建议 UI 层用 **Provider** 聚合多个 **Model entries**（底层仍映射为多个 `[model.*]` 或一个 base_url + 多 model id）：

| 字段 | 说明 |
|------|------|
| `id` / `name` | 显示名，如 “OpenAI”、“公司网关” |
| `base_url` | 必填，OpenAI 兼容根路径 |
| `protocol` | `chat_completions` \| `responses` \| `messages` |
| `auth` | Bearer API key / 自定义 headers / 环境变量名 |
| `models[]` | 可选：手动列表或从 `/models` 拉取 |
| `defaults` | temperature、max tokens、context_window |

#### 5.4.2 协议支持策略

| 协议 | 优先级 | 说明 |
|------|--------|------|
| **OpenAI Chat Completions** | P0 | 兼容面最广（多数中转、Ollama、vLLM） |
| **OpenAI Responses** | P0 | 与原生 grok 默认路径一致（`default_models.json` 中可见 responses）；工具/推理语义更完整的提供商优先 |
| **Anthropic Messages** | P1 | 内核已有；设置向导第二期完整包装 |
| 其它私有协议 | P2 | 仅在用户侧用兼容网关转换，不在 Desktop 内扩张 |

#### 5.4.3 兼容性与降级

| 能力 | 原生 Grok | 典型 Chat Completions 第三方 | Desktop 行为 |
|------|-----------|------------------------------|--------------|
| 文本流式 | ✓ | ✓ | 统一渲染 |
| Tool / function calling | ✓ | 视模型 | 不支持时提示并限制 agent 工具集（或依赖内核已有降级） |
| Reasoning / thinking 块 | ✓ | 部分 | 有则展示，无则隐藏 |
| 多模态输入（图） | ✓ | 部分 | Composer 按模型能力启用附件 |
| 图片/视频生成工具 | 平台能力 | 通常无 | 工具列表过滤或执行失败友好提示 |
| Web search 等托管工具 | 平台 | 本地工具仍可用 | 区分 “平台工具” vs “本地工具” |

#### 5.4.4 安全

- 密钥默认进 **系统凭据库**；config 中写 `env_key` 或 secret ref，避免 TOML 明文  
- `base_url` 允许 http **仅限 loopback**（本地 Ollama）；远程强制 https（可企业开关）  
- SSRF：模型 endpoint 配置属用户信任边界；与 `web_fetch` 工具的 SSRF 策略分离说明  
- Provider 删除时同步清理 keychain 条目  

---

## 6. UI 信息架构（MVP → 完整）

### 6.1 MVP

1. 工作区选择（文件夹 + 信任提示，对齐 folder trust）  
2. 单会话对话 + 流式工具卡片  
3. 权限批准 / 拒绝  
4. 模型选择器（内置 + 已配置自定义）  
5. **添加第三方模型向导**（Chat Completions / Responses）  
6. 登录 SpaceXAI（浏览器）与 API Key  
7. 会话列表 resume  

### 6.2 v1

1. 多 Agent 侧栏 + Dashboard 语义（状态分组、peek、dispatch）  
2. Diff 审阅 / 在外部编辑器打开  
3. 终端面板  
4. MCP / Skills / 权限模式设置  
5. Provider 管理（测试连接、拉模型列表、默认模型）  
6. Plan mode 可视化  

### 6.3 v2+

1. 定时任务 / Inbox（对标 Codex automation）  
2. 多窗口、多 worktree 编排  
3. 远程 agent（现有 `serve` / relay）  
4. 企业托管配置与 MDM 策略透传  

---

## 7. 工程落地建议

### 7.1 目录布局（建议：一体仓 + 可合上游）

```text
grok-build-desktop/                 # 本产品 monorepo（可定期 merge 上游 core）
  crates/codegen/...                # A 类：Grok Build core（对齐上游）
  crates/codegen/xai-grok-pager/    # B 类：TUI；Desktop 运行时不依赖
  apps/
    desktop/                        # C 类：Grok Desktop 一体应用
      src-tauri/                    # Tauri host + ACP bridge + 内置 agent 启停
      src/                          # React UI
      package.json
  packages/
    acp-client/                     # C 类：TS ACP 客户端
  docs/
    DESKTOP_DEV.md
  # 可选：vendor 或 git subtree/submodule 策略见 7.5
```

构建产物：**一个** Desktop 安装包，内含 UI 资源 + host + **同版本 core agent**（侧车二进制或等价嵌入）。

### 7.2 Core 侧增强（优先可回馈上游的小改动）

| 增强 | 目的 | 合并友好 |
|------|------|----------|
| agent 入口稳定（stdio/serve ready 信号） | 一体启动竞态 | 是 |
| `models --json` / 列举字段完备 | Provider UI | 是 |
| config 原子写入 | 设置页安全写 TOML | 是 |
| 模型 probe | 测试连接 | 是 |
| 模型能力位（tools/vision） | UI 降级 | 是 |

Desktop **专用**逻辑（窗口、托盘、品牌）只放 `apps/desktop`，避免污染 A 类 crate。

### 7.3 版本与分发（一体程序）

- **单安装包**交付；core 与 UI **同构建号绑定**（或 UI 显示 `desktop@x / core@y` 且 y 为内置版本）  
- **不**以“检测 PATH 上的 grok”为正式路径；开发可用外置二进制加速迭代  
- 自动更新更新 **整个** Desktop（含内置 core），避免 UI/core 漂移  
- 可选：高级用户仍可安装上游 CLI，与 `~/.grok` 数据互通，但是附加能力  

### 7.4 测试策略

| 层 | 内容 |
|----|------|
| Core | 保持上游式单测；合入上游后先 `cargo test -p` 关键 crate |
| ACP contract | 对内置 agent 入口录制黄金用例 |
| 模型兼容 | Chat Completions / Responses mock |
| Desktop E2E | 一体启动 → 会话 → 权限 → 切模型 |
| **Merge 演练** | 定期 dry-run 合上游 core，修复仅限 adapter/Desktop |

### 7.5 一体包内通信：stdio vs loopback WS vs in-process

对用户都是「一个 Grok Desktop」；差别只在 **UI 宿主与 core agent 怎么说话**。

#### 7.5.1 stdio（子进程 + 标准输入输出）

```text
Tauri host ── spawn 内置 agent ── stdin/stdout ── ACP JSON-RPC
```

| | 说明 |
|--|------|
| **优点** | 与上游 `grok agent stdio` / IDE 路径一致；**无端口、无 secret**；进程退出即断连，状态机简单；安全面最小；最利于「合上游 agent 入口、少写 Desktop 专用传输」 |
| **缺点** | 要处理半包/粘包、stderr 与 stdout 分离（防日志污染 RPC）；UI 热重载常等于重启 agent；多窗口共享同一 agent 不自然 |
| **一体场景** | **默认首选**：安装包内嵌 agent 二进制，host spawn 相对路径即可 |

#### 7.5.2 loopback WebSocket（子进程 + 本机 WS）

```text
Tauri host ── spawn 内置 `agent serve` ── ws://127.0.0.1:port ── ACP
```

| | 说明 |
|--|------|
| **优点** | 消息边界清晰；**断线可重连**（serve 可保留 in-flight）；多窗口连同一内核更顺；开发期可用外部客户端调试 |
| **缺点** | 端口占用、secret、心跳、僵尸进程；误绑非 loopback 有风险；比 stdio 多一层运维代码 |
| **一体场景** | 适合 M4+ 多窗口 /「后台常驻 agent」；**不作为 M1 默认** |

#### 7.5.3 in-process（同进程链入 `MvpAgent` / shell）

```text
Tauri host 线程/runtime ── 内存 channel ── MvpAgent（无独立 agent OS 进程）
```

注意：**in-process 不是第三种 ACP 网络协议**，而是「还要不要子进程」。通道仍可做成与 ACP 同形的消息，便于以后外置。

| | 说明 |
|--|------|
| **优点** | 无 spawn、无端口、延迟最低；调试「单进程」简单；包内不必再带 side-by-side agent 二进制（可减分发复杂度） |
| **缺点** | agent **panic / 死锁 / 阻塞** 可拖垮整个 Desktop；tokio runtime 与 Tauri 生命周期易缠在一起；与上游「agent 入口进程」模型偏离，**合上游时更容易长 Desktop 专用胶水**；崩溃恢复差 |
| **一体场景** | **不作为默认**；仅当明确要极致集成且有严格线程隔离时再评估 |

#### 7.5.4 已确认组合（ADR-7）

| 层级 | 选择 | 原因 |
|------|------|------|
| **进程** | **内置子进程**（非 in-process） | 崩溃隔离；对齐上游 agent 入口；合 core 成本低 |
| **传输** | **stdio 默认** | 简单、安全、与 ACP 生态一致 |
| **抽象** | `AgentTransport` 接口 | 预留 WS，避免业务绑死 |
| **后期** | 可选 loopback WS | 多窗口 / 热重连 |

```text
M1–M3：内置子进程 + stdio
M4+  ：按需加 WS 传输实现；业务代码不改
in-process：默认不做
```

### 7.6 上游代码同步方案对比与选定（ADR-8 已确认）

本仓已是「Grok Build 全树 + Desktop 增量」形态时，同步方式决定长期合入成本。

| 方案 | 做法 | 优点 | 缺点 | 是否推荐 |
|------|------|------|------|----------|
| **A. Git remote + merge** | `git remote add upstream …`；定期 `merge` / `rebase` 上游到本仓 | 历史完整、冲突可解、**日常改 core 与上游同路径**；最贴「可合并上游」目标 | 会带入 pager 等无关路径的冲突，需策略忽略或快速选 upstream | **推荐（默认）** |
| **B. git subtree** | 仅把上游 `crates/` 等以 subtree 拉入 | 边界清晰、可只同步子树 | 命令复杂、双历史易乱、贡献回上游别扭 | 备选（若以后拆成「纯 Desktop 壳仓」） |
| **C. git submodule** | core 独立仓，本仓引用 commit | 上游边界硬 | 日常开发痛苦；IDE/CI 易踩空；一体构建体验差 | **不推荐** |
| **D. 周期 vendor 拷贝** | 手动/脚本复制上游快照 | 简单粗暴 | **几乎无法可持续 merge**；diff 巨大 | **不推荐**（仅紧急热修） |

#### 7.6.1 已确认方案：**A — Upstream remote + 定期 merge**

操作细则见 [`UPSTREAM_SYNC.md`](./UPSTREAM_SYNC.md)。

```text
本仓 (grok-build-desktop)
  apps/desktop/     … Desktop-only，上游不存在
  packages/         … Desktop-only
  docs/             … 可含 Desktop 文档
  crates/...        … 与上游同构，merge 主战场

工作流：
  1. git remote add upstream <Grok-Build-源仓>
  2. 节奏：跟上游 release 或每周 merge（重要安全修复可随时 cherry-pick）
  3. 冲突：
       - crates 内 A 类 core → 优先取 upstream，再修 Desktop adapter
       - xai-grok-pager TUI → 可 theirs/ours 策略，Desktop 不依赖则少纠缠
       - apps/desktop → 永不来自 upstream
  4. merge 后 gate：cargo test 关键 core + Desktop 构建 + ACP smoke
  5. 本地对 core 的必要修复：能 upstream 的尽量 PR 回上游；不能则记 patch 清单，下次 merge 重放
```

**辅助规则（降低冲突）：**

1. **少改 A 类 crate 的格式/无关 churn**（避免无意义 merge 冲突）。  
2. Desktop 需要的能力 → **通用 API** 进 core（可回馈），或 **只动 `apps/desktop` / 薄 host**。  
3. 可选：`.gitattributes` / merge driver 对纯生成物（若有）特殊处理。  
4. 维护 `docs/UPSTREAM_SYNC.md`（可后续补）：上次同步版本、已知 fork 点、跳过路径列表。

#### 7.6.2 何时改用 subtree

- 产品仓希望 **极瘦**（几乎只有 `apps/desktop`），core 完全当外部依赖；  
- 或 upstream 目录布局与本仓严重不一致。  

当前仓库 **已含完整 crates 树** → 继续 **remote merge** 成本最低，不必上 submodule/vendor。

---

## 8. 分阶段里程碑

| 阶段 | 交付 | 退出标准 |
|------|------|----------|
| **M0 评审锁定** | 本文档评审通过；栈选定；ACP 主路径选定 | 签字/共识 |
| **M1 竖切** | 一体 Desktop 启动 + **内置** core agent + 单会话流式 | 不依赖 PATH 上的 grok；Win/mac 可聊一轮 |
| **M2 内核体验** | 权限 UI、会话 resume、工作区信任、原生登录 | 日常替代 TUI 做简单任务 |
| **M3 模型平台** | Provider 向导；**Chat Completions + Responses**；密钥保险箱；连接测试；模型切换 | 可配置 OpenAI/Ollama/中转并完成工具调用任务 |
| **M4 多 Agent** | 侧栏 Dashboard、并行 session、Diff/终端 | 对标 Codex 指挥中心主路径 |
| **M5 打磨发布** | 自动更新、签名、设置页（MCP/Skills）、文档与卸载 | 三平台可分发 RC |

---

## 9. 决策记录（ADR 摘要）

### ADR-1：一体应用 + Core-as-Library/Backend（已确认）

- **决策**：**Grok Desktop 是一体程序**；内嵌 / 同包 Grok Build **非 UI 核心**；不依赖用户已装 CLI。  
- **实现**：UI（Tauri/React）与 core 经 **ACP** 边界通信；对用户单一入口。  
- **原因**：版本一致、可分发、可合并上游 core；避免双 agent 实现。  
- **日期**：2026-07-16。

### ADR-2：UI 技术栈 — Tauri 2 + React（已确认）

- **决策**：**Tauri 2 + React + TypeScript + Vite**；否决 Electron。  
- **原因**：包体/内存、Rust 同仓、系统 API；React 满足 Codex 级 UI。  
- **约束**：业务不锁死在 Tauri API（`packages/acp-client`）。  
- **日期**：2026-07-16。

### ADR-3：第三方模型走现有 Sampler

- **决策**：第三方经 `xai-grok-sampler`（`chat_completions` / `responses` / `messages`）+ 共享 config 约定。  
- **原因**：与上游 core 行为一致，合并不产生“Desktop 专用推理栈”。  

### ADR-4：密钥不进 Git、尽量不进明文 TOML

- **决策**：keychain + `env_key`/secret ref 优先。  

### ADR-5：UI 状态与会话状态分离

- **决策**：会话在 core 持久化路径；Desktop 仅 UI 偏好。  

### ADR-6：上游合并边界 — 只合非 UI Core（已确认）

- **决策**：可持续合并上游 **shell/tools/sampler/workspace/config/…**；**不**以同步上游 TUI（pager）为产品目标。  
- **做法**：A 类 crate 少补丁；Desktop-only 代码隔离在 `apps/desktop`；需要的 core API 变更尽量设计成上游可接受的通用能力。  
- **日期**：2026-07-16。

### ADR-7：一体包内通信 — 子进程 + stdio（已确认）

- **决策**：默认 **内置 agent 子进程 + ACP over stdio**；实现 `AgentTransport` 抽象，WS 可选后加；**默认不做 in-process**。  
- **原因**：对齐上游 agent 入口、崩溃隔离、无端口安全面、合并友好；一体交付不依赖外置 CLI。  
- **非目标**：用户感知「又装了一个服务」——子进程由 Desktop 启停，对用户仍是单应用。  
- **日期**：2026-07-16（产品确认「按推荐定」）。  

### ADR-8：上游代码同步 — git remote + 定期 merge（已确认）

- **决策**：本仓保持完整 core 树；`git remote` 指向上游 Grok Build；**定期 merge**（或关键修复 cherry-pick）。不用 submodule / 不用 vendor 拷贝作主路径。  
- **原因**：与「可合并上游 core」目标一致；本仓已是全树，remote merge 摩擦最小。  
- **备选**：若未来拆成瘦壳仓，再评估 subtree。  
- **细则**：见 [`UPSTREAM_SYNC.md`](./UPSTREAM_SYNC.md)。  
- **日期**：2026-07-16（产品确认「按推荐定」）。  

---

## 10. 开放问题（需产品/工程确认）

1. **品牌与命名**：Grok Desktop 与 Grok Build 的产品名关系  
2. **企业网关**：自定义 CA / 强制代理是否 MVP  
3. **Messages 协议**：是否进 MVP Provider 向导  
4. **弱模型工具策略**：默认收紧 bash 还是仅提示  
5. **是否随包附带可选 CLI 二进制**（高级用户），仍非运行前提  
6. **上游 remote 具体 URL** 与默认同步节奏（建议：跟 release + 紧急 cherry-pick；日常可每周）——占位见 `UPSTREAM_SYNC.md`

---

## 11. 附录

### 11.1 关键 crate 索引

| Crate | 角色 |
|-------|------|
| `xai-grok-pager-bin` | CLI 入口 |
| `xai-grok-pager` | TUI / headless / ACP spawn |
| `xai-grok-shell` | Agent 内核、leader、auth、session |
| `xai-grok-sampler` | 多协议推理客户端 |
| `xai-grok-sampling-types` | `ApiBackend` 等共享类型 |
| `xai-grok-models` | 默认模型目录 |
| `xai-grok-tools` | 工具实现 |
| `xai-grok-workspace` | 工作区、权限、VCS |
| `xai-acp-lib` | ACP 通道与网关 |
| `xai-chat-state` | 会话状态 actor |

### 11.2 自定义模型配置速查

```toml
[model.my-openai-chat]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o Chat"
api_backend = "chat_completions"   # 可省略（默认）
env_key = "OPENAI_API_KEY"

[model.my-openai-responses]
model = "gpt-4o"
base_url = "https://api.openai.com/v1"
name = "GPT-4o Responses"
api_backend = "responses"
env_key = "OPENAI_API_KEY"
```

### 11.3 参考

- 仓内：`README.md`、`crates/codegen/xai-grok-pager/docs/user-guide/`（尤其 11-custom-models、15-agent-mode、17-sessions、23-dashboard）  
- ACP：https://agentclientprotocol.com  
- Codex Desktop 产品：OpenAI Codex app 公告；架构上 CLI-as-backend 模式可对标  
- xAI 文档：https://docs.x.ai/build/overview  

---

## 12. 下一步

1. ~~Tauri / 一体 / stdio / 同步~~ → 均已确认（ADR-1/2/7/8）。  
2. 填写 [`UPSTREAM_SYNC.md`](./UPSTREAM_SYNC.md) 中的 **upstream remote URL**（有源仓地址后）。  
3. **M1**：`apps/desktop` 骨架 + **内置 agent 子进程 + stdio ACP** + 单会话流式。  
4. 并行：可回馈上游的 models/probe 小改动。  
5. 余下产品项见 §10（命名、Messages 向导等）。  

**一句话总结：** 一体 **Grok Desktop**（Tauri + React）内嵌 Grok Build **非 UI core**；已确认 **子进程 + ACP/stdio**、**git upstream remote 定期 merge**；WS 后加；in-process / submodule / vendor 不作主路径。
