# N-coding 工作阶段划分

> 基于 `docs/narwhal_dev.md` 中的命令系统和架构设计。每个阶段按目录/模块拆分具体任务。

---

## Phase 1: 最小可用原型（MVP） ✅ 完成

**目标**：能用 TUI 与 DeepSeek 对话，Shell 命令可同步执行。

### 已实现功能

| 模块 | 文件 | 实现内容 |
|------|------|---------|
| `src/config/` | `loader.rs` | KDL 配置加载，三级优先级合并（`.ncoding/n_coding.kdl` > `~/.config/ncoding/config.kdl` > 默认值），robust 解析 |
| `src/api/` | `client.rs` | DeepSeek API SSE 流式，reasoning_content 分离回传，双通道输出（TUI + CommandWatcher），`list_models()`，`generate_session_name()` |
| `src/tui/` | `app.rs` | 5区布局（标题/文本/Token/输入/状态），Everforest 主题，流式渲染，UTF-8 + 中文输入，自动滚动，slash 指令 |
| | `theme.rs` | Everforest 暗色主题色彩常量 |
| `src/command/` | `parser.rs` | `<<<[Command]>>>` 正则解析器，5→7 种命令类型，`---` 分隔、`__END__` 截止、命令名标准化 |
| | `syntax.rs` | 数据结构：NCommand, ShellBlock, FileOpBlock, ToolCallBlock, SubAgentBlock, SkillsBlock, CheckListBlock, AgentLogsBlock, CommandResult |
| | `shell.rs` | nu 同步/异步执行 (tokio::process)，安全审查，输出截断(>100行) |
| | `files_operator.rs` | Read(行号+offset/limit)、Write、Edit(精确替换+唯一性检查) |
| | `mod.rs` | CommandWatcher（流式监听+中流提取+非阻塞收集），命令调度，结果格式化 |
| `src/prompt/` | `builder.rs` | 系统提示词：character + grammar + 7 种命令文档 + 外部工具 |
| `src/main.rs` | | 日志落盘 `.ncoding/n-coding.log`(重置)，完整集成 |

### 对话循环

```
用户输入 → add user msg → continue_conversation()
  ↓
API SSE stream → TUI 流式显示 + CommandWatcher 中流监听
  ↓
StreamDone → save assistant msg → tokio::spawn(drain_results) → 非阻塞
  ↓
CommandsCompleted → format results → inject user msg → auto continue (≤20次)
  ↓
无命令且无未完成 CheckList → AppState::Stop
```

### 命令系统特性

- **双向性**：assistant 流式输出中识别命令；user 直接输入纯命令时直接执行不调 API
- **即时性**：中流遇到 `<<<[__END__]>>>` 立即 spawn 执行，无需等模型输出结束
- **非阻塞**：`drain_results` 在后台 tokio task 中收集，不阻塞 TUI 事件循环
- **参数完整**：中流解析仅在有下一标记符（`__END__` 或下一个命令）时才构建命令，防止参数不完整

---

## Phase 2: 命令体系完善 ✅ 完成

| 步骤 | 任务 | 状态 |
|------|------|------|
| 2.1 | Shell — 同步/异步执行，安全审查(sudo/rm-rf/chmod/dd)，输出截断 | ✅ |
| 2.2 | Shell — `find` 命令 10s 短超时 | ✅ |
| 2.3 | FilesOperator — Read（行号+offset/limit）、Write（创建/覆盖）、Edit（精确替换） | ✅ |
| 2.4 | ToolCall — 外部工具调用，KDL 配置，CLI-arg JSON 传参，并行执行 | ✅ |
| 2.5 | SubAgentTask — 独立上下文任务委派 | ✅ |
| 2.6 | AgentSkills — list/load，工作目录+全局目录扫描 | ✅ |
| 2.7 | **CheckList** — 任务规划器，create/update/list，`.ncoding/checklist.json`，未完成任务自动继续 | ✅ |
| 2.8 | **AgentLogs** — 开发日志，write/read/list，`.ncoding/agent_logs/` | ✅ |
| 2.9 | 提示词模板 — 7 种命令完整文档 | ✅ |
| 2.10 | CommandWatcher — 中流提取 + 非阻塞收集 | ✅ |

### 命令执行流程

```
模型输出 token → feed_token(buffer)
  ↓
extract_commands_from (non-final) → 有 __END__ 时解析 body
  ↓
build_command → NCommand variant
  ↓
tokio::spawn(execute_commands) → 异步执行
  ↓
drain_results (background task) → 收集 CommandResult
  ↓
format_command_results → <(<(SYSTEM...[Result])>)>
```

---

## Phase 3: Session + 日志 + 状态 ✅ 完成

| 步骤 | 任务 | 状态 |
|------|------|------|
| 3.1 | Session JSON 持久化（`.ncoding/sessions/<name>.json`） | ✅ |
| 3.2 | Session 自动命名（首次输入 → flash API → slug） | ✅ |
| 3.3 | Session 切换、重命名、删除、列表 | ✅ |
| 3.4 | `/undo` 撤回机制（轮次级） | ✅ |
| 3.5 | `/clear`、`/help`、`/quit` 指令 | ✅ |
| 3.6 | Token 信息栏（usage 解析 + cache hit 比例） | ✅ |
| 3.7 | 上下文截断（max_context_messages） | ✅ |
| 3.8 | 日志系统 — info 级别，API 请求/命令解析/命令执行记录 | ✅ |
| 3.9 | 状态显示 — STOP(绿) / WORKING(红) 在标题栏右侧 | ✅ |
| 3.10 | 输入 guard — 命令执行中阻止新用户输入 | ✅ |

---

## Phase 4: 打磨与完善 ⬜ 进行中

| 步骤 | 任务 | 状态 |
|------|------|------|
| 4.1 | `/model` 面板切换 | ⬜ |
| 4.2 | Markdown 渲染完善（表格、链接、图片） | ⬜ |
| 4.3 | 命令块/结果 折叠展开 UI | ⬜ |
| 4.4 | 终端 resize 自适应 | ⬜ |
| 4.5 | Shell 环境信息注入（ShellEnvInfo + build_env_injection） | ⬜ |
| 4.11 | 命令调用的双向性增强（用户消息中识别命令） | ⬜ |
| 4.12 | `$` 命令行方式启动方案 | ⬜ |

---

## 阶段依赖关系

```
Phase 1 (MVP) ✅
    │
    ├─→ Phase 2 (命令体系 8 种命令) ✅
    │
    ├─→ Phase 3 (Session + 日志 + 状态) ✅
    │
    └─→ Phase 4 (打磨 + narwhal 架构)
            ├─ UI 增强 (model 面板, markdown, resize, 折叠)
```

---

## 目录 → 阶段 速查表

| 目录 | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|------|:---:|:---:|:---:|:---:|
| `src/config/` | ★ | | | |
| `src/api/` | ★ | | ★ | |
| `src/tui/` | ★ | | ★ | ★ |
| `src/command/` | ★ | ★ | ★ | ★ |
| `src/session/` | | | ★ | ★ |
| `src/prompt/` | ★ | ★ | | |

> ★ = 已完成，⬜ = 未开始

---

## 命令速查

| 命令 | 参数 | 用途 | 文件 |
|------|------|------|------|
| Shell | command, is_async | nushell 命令执行 | `shell.rs` |
| FilesOperator | mode(read/write/edit), path, content, old_str, new_str, offset, limit | 文件读写编辑 | `files_operator.rs` |
| ToolCall | tool_name, args... | 外部工具调用(CLI-arg JSON) | `tool_call.rs` |
| SubAgentTask | prompt | 子 agent 任务委派 | `sub_agent_task.rs` |
| AgentSkills | mode(list/load), skill_name | 技能发现与加载 | `agent_skills.rs` |
| CheckList | mode(create/update/list), id, title, status, content | 任务规划与跟踪 | `checklist.rs` |
| AgentLogs | mode(write/read/list), filename, content | 开发日志读写 | `agent_logs.rs` |

---

## 技术债 / 待清理

| 项目 | 说明 |
|------|------|
| `src/tui/input.rs` | InputState 模块未使用，app.rs 内联处理输入 |
| `src/tui/model_panel.rs` | ModelPanel 未使用，Phase 4 中实现 |
| `src/tui/render.rs` | MarkdownRenderer 未使用，Phase 4 中完善 |
| `src/session/backup.rs` | BackupManager 未使用，Phase 3 `/undo` 内联实现 |
| `src/api/client.rs` | `list_models`、`generate_session_name` 通过 `#[allow(dead_code)]` 保留 |
| `src/prompt/builder.rs` | `build_env_injection`、`ShellEnvInfo` 预留给 Phase 4.5 |
