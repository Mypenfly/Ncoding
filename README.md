# N-coding

> 一个基于 TUI 的 coding agent，专为 DeepSeek API 设计。模型通过文本命令系统操控你的终端和文件系统——边思考边执行，不间断。

[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## 什么是 N-coding？

N-coding 是一个运行在终端里的 AI 编程助手。与典型的 chat 工具不同，它不依赖 OpenAI 风格的 `tool_call`/`function_call`，而是定义了一套**文本命令语法**。模型在流式输出 token 的同时，你就能看到 Shell 命令被解析并异步执行，无需等待生成完成。

**核心理念**：让模型"边想边做"（reasoning + acting 并发流式），而非"先想完再一次性调用工具"。

---

##特性

- **文本命令系统** — 7 种内置命令：Shell、FilesOperator、ToolCall、SubAgentTask、AgentSkills、CheckList、AgentLogs。模型输出中即时解析、非阻塞执行。
- **DeepSeek 深度集成** — 完整支持 `reasoning_content` 往返，thinking mode（`reasoning_effort` 可配），SSE 流式响应。
- **TUI 界面** — ratatui + crossterm，Everforest 暗色主题，UTF-8 中文输入，无限滚动，Token 用量显示。
- **Session 管理** — JSON 持久化，自动命名，多会话切换，`/undo` 撤回。
- **3 级配置合并** — 环境变量 → 项目根 `n_coding.kdl`（KDL 格式）→ `~/.config/ncoding/config.kdl`。
- **安全执行** — Shell 命令安全审查（禁 sudo/rm -rf/chmod/dd），输出截断，find 短超时。
- **开发辅助** — CheckList 任务追踪、AgentLogs 开发日志、自定义 Skills。
- **Nix flake** — 开箱即用的开发环境，内置 Rust 工具链、nushell、ripgrep、jujutsu。

---

## 快速开始

### 前置要求

- Rust stable（通过 [rustup](https://rustup.rs) 或 Nix）
- `DEEPSEEK_API_KEY` 环境变量
- 终端支持 true color（推荐 kitty / wezterm / Windows Terminal）

### 使用 Nix（推荐）

```bash
git clone https://github.com/your-org/n-coding.git
cd n-coding
nix develop    # 或 direnv allow，自动进入 nushell
cargo build
DEEPSEEK_API_KEY=sk-xxx cargo run
```

### 不使用 Nix

```bash
# 需要：Rust stable + bash + ripgrep (rg) + jujutsu (jj)
git clone https://github.com/your-org/n-coding.git
cd n-coding
cargo build --release
DEEPSEEK_API_KEY=sk-xxx ./target/release/n-coding
```

---

## 命令系统

模型通过以下语法调用命令（在流式输出中即时解析）：

```
《[CommandName]|character|》
【key1】value1【key1】
【key2】value2【key2】
《[End]|CommandName|》
```

### 内置命令一览

| 命令 | 用途 |
|------|------|
| **Shell** | 执行 bash 命令（同步/异步），安全审查，输出截断 |
| **FilesOperator** | 文件的 Read（行号+offset/limit）、Write、Edit（精确字符串替换） |
| **ToolCall** | 调用 KDL 配置中定义的外部工具，CLI-arg JSON 传参 |
| **SubAgentTask** | 委派子任务到独立上下文 agent |
| **AgentSkills** | 技能发现（list）与加载（load），支持项目/全局目录 |
| **CheckList** | 任务规划器：create / update / list |
| **AgentLogs** | 开发日志：write / read / list |

更多细节见 [`docs/ncoding.md`](docs/ncoding.md)。

---

## 项目结构

```
src/
├── main.rs          # 入口：日志初始化 + TUI 启动
├── api/             # DeepSeek API SSE 流式客户端
├── command/         # 命令解析器 + 7 种执行器
├── config/          # KDL 配置加载 (3 级合并)
├── prompt/          # 系统提示词模板构建
├── session         # Session JSON 持久化 + 上下文管理
└── tui/             # ratatui 界面：app、render、input、theme
```

各模块详情见 `src/*/README.md`。

---

## 开发

```bash
nix develop          # 进入开发环境 (nushell)
cargo build          # 构建
cargo test           # 测试（推荐 cargo nextest run）
cargo clippy -- -D warnings   # Lint
cargo fmt -- --check           # 格式检查
```

遵循 TDD 流程：先写测试 → 确认失败 → 实现 → 通过 → 重构。详见 [`AGENTS.md`](AGENTS.md)。

---

## 开发阶段

| 阶段 | 状态 | 完成率 |
|------|------|--------|
| Phase 1: MVP — TUI 对话 + Shell 同步执行 | ✅ 完成 | 100% (9/9) |
| Phase 2: 命令体系 — 7 种内置命令 | ✅ 完成 | 100% (7/7) |
| Phase 3: Session + Shell 进阶 | ✅ 基本完成 | 80% (8/10) |
| Phase 4: 打磨 — model 面板、折叠、resize、markdown 渲染 | 🚧 进行中 | 30% (2/7) |

**当前状态**: 150 测试全部通过。核心命令系统、session 管理、API 集成均已完成。
Phase 4 优先项: 终端 resize 自适应 → /model 面板 → Markdown 渲染完善 → 命令块折叠。

详见 [`phases.md`](phases.md)。

---

## 配置示例 (`n_coding.kdl`)

```kdl
api {
    model "deepseek-v4-pro"
    api_key_env "DEEPSEEK_API_KEY"
    max_tokens 65536
    temperature 0.0
}
thinking {
    reasoning_effort "high"
}
session {
    max_context_messages 50
}
safety {
    allowed_shell_commands "cargo" "git" "rg" "jj" "ls" "cat" "echo"
    blocked_shell_patterns "rm -rf" "sudo" "chmod 777" "dd if="
}
```

---

## 文档

- [`docs/ncoding.md`](docs/ncoding.md) — 完整设计文档
- [`docs/narwhal_dev.md`](docs/narwhal_dev.md) — 命令系统草案
- [`docs/tool_call_dev.md`](docs/tool_call_dev.md) — 外部工具开发指南
- [`AGENTS.md`](AGENTS.md) — 开发者指南
- [`phases.md`](phases.md) — 开发阶段与进度

---

## License

MIT
