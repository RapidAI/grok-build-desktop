# Grok Build Desktop

[English README](README.md)

Grok Build Desktop 是一个面向编程任务的 Agent 桌面程序，基于
[Grok Build](https://github.com/xai-org/grok-build) 移植并构建。项目将
Grok Build 的 Agent 运行时打包进 Tauri 桌面应用，通过原生图形界面提供
AI 编程助手能力。

桌面应用代码位于 [`apps/desktop`](apps/desktop/)；本仓库同时保留 Grok
Build 上游的 CLI/TUI 源码与运行时，并会定期同步。

## 桌面应用架构

- `apps/desktop/src/`：React 桌面界面
- `apps/desktop/src-tauri/`：Tauri 主机层，负责 Agent 生命周期、ACP 客户端、配置、密钥与工作区信任
- `packages/acp-client/`：共享的 NDJSON ACP 辅助库
- 内置 Agent 与桌面端通过标准输入输出上的 Agent Client Protocol（ACP）通信

## 本地开发

```bash
cd apps/desktop
npm install
npm run desktop:dev
```

开发时可通过 `GROK_DESKTOP_AGENT_PATH` 指向自定义 Agent 二进制文件。

## 测试与构建

```bash
cd apps/desktop/src-tauri
cargo test
cd ..
npm run test:unit
npm run desktop:build
```

## Grok Build 上游项目

本仓库包含并定期同步 Grok Build 的 Rust CLI/TUI 与 Agent 运行时源码。
有关上游 Grok Build 的安装、构建、开发与许可信息，请参阅
[英文 README](README.md) 中的“Upstream Grok Build”部分。
