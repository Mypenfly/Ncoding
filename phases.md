# N-coding 工作阶段划分

> 基于 `docs/ncoding.md` 第 11 节开发路线图，每个阶段内部按目录/模块拆分具体任务。

---

## Phase 1: 最小可用原型（MVP） ✅ 完成

**目标**：能用 TUI 与 DeepSeek 对话，Shell 命令可同步执行。

### 已实现功能

| 模块 | 文件 | 实现内容 |
|------|------|---------|
| `src/config/` | `loader.rs` | KDL 配置加载，三级优先级合并（工作目录 `.ncoding/n_coding.kdl` > 全局 `~/.config/ncoding/config.kdl` > 默认值），robust 解析（完整文档解析失败时回退到逐节解析） |
| `src/api/` | `client.rs` | DeepSeek API SSE 流式客户端，reasoning_content 与 content 分离并正确拼接，stream_chat 双通道输出（TUI + CommandWatcher），`list_models()` 模型列表，`generate_session_name()` session 命名 |
| `src/tui/` | `app.rs` | 完整 TUI：5 区域布局（标题栏/文本区/Token栏/输入框/状态栏），Everforest 配色，流式渲染（reasoning 灰、content 正文），自动滚动，UTF-8 中文输入支持，光标显示，slash 指令，SessionManager 集成 |
| | `input.rs` | `InputState`，中文字符安全操作（byte-boundary-aware 插入/删除/移动） |
| | `render.rs` | Markdown 基础渲染（代码块/标题/内联代码） |
| | `theme.rs` | Everforest 暗色主题色彩常量 |
| `src/command/` | `parser.rs` | `<<<[Command]>>>` 正则解析器（5种命令类型块提取、`---` 分隔、`__END__` 截止、命令名标准化） |
| | `syntax.rs` | 数据结构：NCommand, ShellBlock, FileOpBlock, ToolCallBlock, SubAgentBlock, SkillsBlock, CommandResult |
| | `shell.rs` | nu 同步/异步执行，安全审查（sudo/rm-rf/chmod/dd 拦截），输出截断（>100行保留头尾），PATH 查找 nu |
| | `files_operator.rs` | Read（带行号/offset/limit）、Write（创建/覆盖）、Edit（精确字符串替换+唯一性检查） |
| | `tool_call.rs` | 外部工具调用：从 KDL 配置读取工具定义，通过环境变量 `NCODING_TOOL_ARGS`(JSON) 传递参数，支持任意语言（Python/Shell/等） |
| | `mod.rs` | CommandWatcher（流式 buffer 监听+解析），命令执行调度，结果格式化注入 `<(<(SYSTEM ... )>)>` |
| `src/prompt/` | `builder.rs` | 系统提示词组装：character + command_grammar + shell + files_operator + 工具定义 |
| `src/main.rs` | | 日志重定向至 `.ncoding/n-coding.log`，完整集成布线 |

### 对话循环

```
用户输入 → add user msg → continue_conversation()
  ↓
API stream → TUI 流式显示 + CommandWatcher 监听
  ↓
API 结束 → add assistant msg (reasoning + content)
  ↓
命令? → execute_commands() → format → inject user msg → auto continue (最多 5 次)
  ↓
回到 TUI 等待输入
```

### 命令系统

```
build: cargo build        # 0 warnings
test:  cargo test         # 74 passed
lint:  cargo clippy -- -D warnings  # clean
```

---

## Phase 2: 完善命令体系 ✅ 部分完成

| 步骤 | 任务 | 状态 |
|------|------|------|
| 2.1 | FilesOperator - Read（文件读取 + 行号返回） | ✅ |
| 2.2 | FilesOperator - Write（创建/覆盖文件） | ✅ |
| 2.3 | FilesOperator - Edit（精确字符串替换 + 唯一性检查） | ✅ |
| 2.4 | ToolCall 外部工具调用（KDL 配置定义 + 环境变量传参） | ✅ |
| 2.5 | SubAgentTask（独立上下文，禁止循环） | ⬜ 占位 (Phase 3) |
| 2.6 | AgentSkills（list/load，工作目录 + 全局目录扫描） | ⬜ 占位 (Phase 3) |
| 2.7 | 提示词模板 | ✅ |

---

## Phase 3: Session + TUI 交互完善 ✅ 部分完成

| 步骤 | 任务 | 状态 |
|------|------|------|
| 3.1 | Session JSON 文件读写（`.ncoding/sessions/`） | ✅ |
| 3.2 | Session 自动命名（首次输入 → flash API → slug） | ✅ |
| 3.3 | Shell is_async 模式（后台执行 + 即时返回） | ⬜ 占位 |
| 3.4 | Shell 安全审查完善 | ✅ |
| 3.5 | Slash 指令：`/session list\|switch\|rename\|delete\|current`，`/undo`，`/help`，`/clear`，`/quit` | ✅ |
| 3.6 | `/model` 切换面板 | ⬜ 占位 |
| 3.7 | Session 切换上下文加载 | ✅ |
| 3.8 | `/undo` 撤回机制 | ✅ |
| 3.9 | Token 信息栏（usage 解析） | ✅ |
| 3.10 | 终端 resize 自适应 | ⬜ |

### Session 管理功能

- **持久化**: `.ncoding/sessions/<name>.json`
- **自动命名**: 首次输入 → `generate_session_name()` 通过 flash API 生成英文 slug
- **Slash 指令**: `/session list` (列表), `/session switch` (切换), `/session rename` (重命名), `/session delete` (删除), `/session current` (当前信息), `/undo` (撤轮)
- **上下文截断**: `max_context_messages` 限制，保留 system prompt + 最近 N 条
- **状态栏**: 显示当前 session 名称

---

## Phase 4: 打磨与完善

| 步骤 | 任务 | 状态 |
|------|------|------|
| 4.1 | Markdown 渲染完善 | ⬜ |
| 4.2 | 命令块/结果 折叠展开 | ⬜ |
| 4.3 | 错误处理完善 | ⬜ |
| 4.4 | 日志系统 | ✅ |
| 4.5 | Shell 环境信息注入 | ⬜ |
| 4.6 | 大文件 read 截断提示 | ⬜ |

---

## 阶段依赖关系

```
Phase 1 (MVP) ✅
    │
    ├─→ Phase 2 (命令体系) ✅ 主要完成
    │
    ├─→ Phase 3 (Session + TUI) ✅ 主要完成
    │
    └─→ Phase 4 (打磨) ⬜
```

---

## 目录 → 阶段 速查表

| 目录 | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|------|:---:|:---:|:---:|:---:|
| `src/config/` | ★ | | | |
| `src/api/` | ★ | | ☆ | |
| `src/tui/` | ★ | ☆ | ★ | |
| `src/command/` | ★ | ★ | ☆ | ★ |
| `src/session/` | | | ★ | |
| `src/prompt/` | ★ | ★ | | |

> ★ = 已完成，☆ = 部分完成，⬜ = 未开始
