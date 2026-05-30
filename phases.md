# N-coding 开发阶段

> 最后更新: 2026-05-31 06:00 UTC

---

## Phase 1: 最小可用原型 (MVP) ✅ 完成

**目标**: TUI 对话 + Shell 同步执行

| # | 任务 | 状态 | 备注 |
|---|------|------|------|
| 1.1 | `cargo init`，Rust 项目骨架 | ✅ | ratatui, reqwest, tokio, serde, kdl-rs, regex |
| 1.2 | KDL 配置加载器 (3级合并) | ✅ | `config/loader.rs` |
| 1.3 | DeepSeek API SSE 流式客户端 | ✅ | `api/client.rs` — reasoning_content 往返, usage 解析 |
| 1.4 | 基础 TUI (ratatui + crossterm, Everforest 主题) | ✅ | `tui/app.rs`, `tui/theme.rs` |
| 1.5 | CommandParser 引擎 (字节级扫描 + 深度追踪 + 栈式KV) | ✅ | `command/parser.rs`, `command/syntax.rs` |
| 1.6 | Shell 命令执行 (bash -c, 安全审查, 同步, 超时) | ✅ | `command/shell.rs` |
| 1.7 | CommandWatcher (流式监听+中流提取+异步分发) | ✅ | `command/mod.rs` |
| 1.8 | 命令结果系统软注入 + 下一轮 API 串联 | ✅ | `command/mod.rs` |
| 1.9 | 端到端集成测试 | ✅ | 150 tests passing |

---

## Phase 2: 完善命令体系 ✅ 完成

**目标**: 7 种内置命令全部实现

| # | 任务 | 状态 | 备注 |
|---|------|------|------|
| 2.1 | FilesOperator Read (行号+offset/limit) | ✅ | `command/files_operator.rs` |
| 2.2 | FilesOperator Write (创建/覆盖 + 原子写入 + 验证) | ✅ | 含 backup 机制 |
| 2.3 | FilesOperator Edit (行匹配 + `...` 省略, 忽略空格匹配) | ✅ | 参考 Aider 匹配策略 |
| 2.4 | ToolCall (外部工具, CLI-arg JSON 传参, 并发调用) | ✅ | `command/tool_call.rs` |
| 2.5 | SubAgentTask (独立上下文, 禁止递归) | ✅ | `command/sub_agent_task.rs` |
| 2.6 | AgentSkills (list/load, 工作目录+全局搜索) | ✅ | `command/agent_skills.rs` |
| 2.7 | 系统提示词模板 (含全部命令语法) | ✅ | `prompt/builder.rs` |

---

## Phase 3: Session + Shell 进阶 🚧 90% 完成

**目标**: 完整 session 管理 + 命令体验 + slash 指令

| # | 任务 | 状态 | 备注 |
|---|------|------|------|
| 3.1 | Session JSON 持久化 | ✅ | `.ncoding/sessions/*.json` |
| 3.2 | 自动命名 (DeepSeek API → slug) | ✅ | `session/manager.rs |
| 3.3 | Shell is_async 模式 | ✅ | `command/shell.rs` — async 执行 + output capture |
| 3.4 | Shell 安全审查完善 | ✅ | sudo/rm/chmod/dd 拦截 |
| 3.5 | Slash 指令路由 (/help /clear /session /undo /quit) | ✅ | `tui/app.rs` |
| 3.6 | /model 面板 (模型切换 + thinking 控制 + /model list) | 🔴 未实现 | `model_panel.rs` 有骨架但 `#![allow(dead_code)]`，slash 返回 "coming in Phase 3" |
| 3.7 | Session 切换 + 上下文截断 | ✅ | max_context_messages 可用 |
| 3.8 | /undo 撤回机制 | ✅ | backup + remove_last_turn |
| 3.9 | Token 信息栏 (usage + cache hit + 实时金额) | ✅ | SSE chunk usage 解析 + 显示 |
| 3.10 | 终端 resize 自适应 (SIGWINCH) | 🔴 未实现 | 无信号处理，无布局重算——**Phase 4 优先** |

---

## Phase 4: 打磨 🚧 30% 完成

**目标**: 优秀的 TUI 体验和完整 polish

| # | 任务 | 状态 | 备注 |
|---|------|------|------|
| 4.1 | Markdown 渲染完善 (代码语法高亮, 表格, 标题层级) | 🟡 基础可用 | `render.rs` 有骨架但 `dead_code`，app.rs 有内联基础渲染 |
| 4.2 | 命令块折叠/展开 (Tab 切换) | 🔴 未实现 | draw() 中无折叠逻辑 |
| 4.3 | API 错误重试 + 网络超时处理 | 🔴 未实现 | 无重试逻辑 |
| 4.4 | 日志系统 (tracing, .ncoding/n-coding.log) | ✅ | `main.rs` 初始化 |
| 4.5 | Shell 环境信息注入 system prompt | ✅ | `prompt/builder.rs` |
| 4.6 | 大文件 read 截断提示友好化 | ✅ | 2000 行限制 + 提示信息 |
| 4.7 | Prompt 中 Nushell → Bash 语法切换 | ✅ | 已完成切换，使用 `bash -c` |

---

## 下一步优先级 (Phase 4)

按用户价值和技术依赖排序：

### P0: 基础体验修复
1. **终端 resize 自适应** — SIGWINCH 信号监听，布局重算。zellij 分屏必做。
2. **/model 面板** — 模型切换 + thinking on/off + reasoning_effort 切换 + /model list 从 API 获取

### P1: TUI 体验提升
3. **Markdown 渲染完善** — 整合 render.rs 的 MarkdownRenderer，支持代码块语法高亮、标题层级、嵌套列表
4. **命令块折叠/展开** — Tab 键交互，折叠状态记忆  **(复杂度较高，依赖解析块边界)**

### P2: 健壮性
5. **API 错误重试** — 指数退避，可配置重试次数
6. **BackupManager 整合** — 将 backup.rs 的完整逻辑接入 manager 和 files_operator
7. **Ctrl+K 终止命令** — 当前未实现
8. **/cancel (Esc) 优化** — 当前仅发送 OutputEnded 不走完整 cancel 流程

---

## 统计

| 阶段 | 完成率 | 测试 |
|------|--------|------|
| Phase 1 | 100% (9/9) | ✅ |
| Phase 2 | 100% (7/7) | ✅ |
| Phase 3 | 80% (8/10) | ✅ |
| Phase 4 | 30% (2/7) | 🟡 |

**总计**: 26/33 完成 (79%), 150 测试全部通过
