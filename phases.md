# N-coding 工作阶段划分

> 基于 `docs/ncoding.md` 第 11 节开发路线图，每个阶段内部按目录/模块拆分具体任务。

---

## Phase 1: 最小可用原型（MVP）

**目标**：能用 TUI 与 DeepSeek 对话，Shell 命令可同步执行。

### 涉及目录

| 目录 | README | 阶段任务 |
|------|--------|---------|
| `src/config/` | [README](src/config/README.md) | 实现 KDL 配置加载器（`loader.rs`），支持工作目录 + 全局目录 + 环境变量三级优先级合并 |
| `src/api/` | [README](src/api/README.md) | 实现 DeepSeek API 客户端（`client.rs`），SSE 流式响应解析，reasoning_content 与 content 分离 |
| `src/tui/` | [README](src/tui/README.md) | 基础 TUI 搭建：单页文本区无限滚动 + 输入框 + 状态栏 + 标题栏，Everforest 配色（`app.rs`, `render.rs`, `theme.rs`, `input.rs` 基础） |
| `src/command/` | [README](src/command/README.md) | `parser.rs`（正则解析器 + 分隔符 `---` + 命令名标准化）、`syntax.rs`（数据结构 enum）、`shell.rs`（nu -c 同步执行 + 安全审查）、`mod.rs`（CommandWatcher 流式 buffer 监听 + 即时解析 + 异步执行 + OutputEnded 汇总） |
| `Cargo.toml` | — | 引入依赖：ratatui, crossterm, tokio, reqwest, serde, regex, knus, chrono, futures 等 |

### 产出物

- 可启动 TUI，发起一次 coding 请求
- 模型调用 Shell 命令同步执行（`is_async = false`）
- 命令结果通过系统软注入返回给模型，继续对话
- 配置可从 KDL 文件加载

### 验证标准

端到端手动测试：发一条 coding 请求 → 模型调用 Shell 执行 → 结果返回 → 继续对话

---

## Phase 2: 完善命令体系

**目标**：FilesOperator + ToolCall + SubAgentTask + AgentSkills + 提示词模板。

### 涉及目录

| 目录 | README | 阶段任务 |
|------|--------|---------|
| `src/command/` | [README](src/command/README.md) | `files_operator.rs`（Read 带行号 / Write 创建覆盖 / Edit 精确字符串替换+唯一性检查）、`tool_call.rs`（外部工具调用）、`sub_agent_task.rs`（独立上下文+禁止循环）、`agent_skills.rs`（list/load+双路径扫描） |
| `src/prompt/` | [README](src/prompt/README.md) | `builder.rs` 完整实现：character_prompt + command_grammar + shell_prompt + files_operator_prompt + sub_agent_task_prompt + agent_skills_prompt + tool_call_prompt + tools_prompts，每个命令附带用法示例和约束说明 |

### 产出物

- 5 种内置命令全部可工作
- 系统提示词自动组装
- FilesOperator edit 唯一性检查正常工作

### 验证标准

模型能正确调用 Read → Edit 流程修改文件；能调用 ToolCall 执行外部工具；能通过 AgentSkills 加载技能。

---

## Phase 3: Session + Shell 进阶 + TUI 交互完善

**目标**：完整的 session 管理 + Shell async 模式 + 所有 Slash 指令和快捷键。

### 涉及目录

| 目录 | README | 阶段任务 |
|------|--------|---------|
| `src/session/` | [README](src/session/README.md) | `manager.rs`：Session JSON 读写 + 自动命名（首次输入→flash API→slug）+ 切换/重命名/删除/列表 + 上下文截断策略 + `/undo` 撤轮逻辑；`backup.rs`：文件备份与恢复 |
| `src/command/` | [README](src/command/README.md) | `shell.rs`：async 模式完善（后台执行 + 即时返回 + 完成回调 + 输出修剪 + 安全审查 deny_patterns） |
| `src/tui/` | [README](src/tui/README.md) | `input.rs`：Slash 指令解析与路由（`/help /clear /undo /model /session /skills /cancel /quit`）；`model_panel.rs`：`/model` 切换面板 + `/model list` 实时获取模型列表；`app.rs`：Token 信息栏（usage 解析 + 缓存命中率 + 金额统计）、终端 resize 自适应（SIGWINCH） |
| `src/api/` | [README](src/api/README.md) | `list_models()` → 实时获取模型列表；`generate_session_name()` → session 自动命名 |

### 产出物

- Session 完整生命周期管理（创建、切换、持久化、截断）
- Shell async 命令（后台执行 + 完成通知）
- 所有 Slash 指令可交互操作
- Token 计费统计实时显示
- 终端 resize 自适应

### 验证标准

多 session 间切换不发生上下文混乱；async 命令在后台执行完成后结果正确注入；终端 resize 时布局正确重新计算。

---

## Phase 4: 打磨与完善

**目标**：Markdown 渲染完善 + 折叠展开 + 错误处理 + 日志 + 细节优化。

### 涉及目录

| 目录 | README | 阶段任务 |
|------|--------|---------|
| `src/tui/` | [README](src/tui/README.md) | `render.rs`：Markdown 渲染完善（代码块语法高亮、标题、列表、表格、内联代码、加粗斜体）+ 命令块/结果折叠展开（Tab 键交互，记忆折叠状态） |
| `src/command/` | [README](src/command/README.md) | `files_operator.rs`：大文件 read 截断提示友好化 + edit 错误信息完善；全局错误处理规范 |
| `src/main.rs` | — | 日志系统（`.ncoding/n-coding.log`, 分级日志, tracing subscriber） |
| `src/prompt/` | [README](src/prompt/README.md) | Shell 环境信息注入到 system prompt（告知 nushell、rg、jj 可用 + more info） |

### 产出物

- 完善 markdown 渲染
- 命令块 Tab 折叠/展开交互
- API 错误自动重试
- 分级日志记录
- 代码整体健壮性提升

### 验证标准

所有 markdown 格式正确渲染；Tab 键折叠/展开流畅；异常情况有合理的错误提示和日志记录。

---

## 后续可扩展（非 MVP）

| 功能 | 说明 |
|------|------|
| Async 命令历史查看 (`/jobs`) | 查看正在执行和已完成的后台命令 |
| Skills 热加载 | watch 文件变更自动重载 skills |
| 鼠标滚动支持 | 支持鼠标滚轮滚动文本区 |
| Session 内搜索 (`/search`) | 搜索对话历史 |
| 多 agent character 支持 | 支持配置文件中定义多个角色 |
| 非 DeepSeek provider 兼容 | 支持 OpenAI、Anthropic API 格式 |

---

## 阶段依赖关系

```
Phase 1 (MVP)
    │
    ├─→ Phase 2 (命令体系)  ─── 依赖 Phase 1 的 parser + CommandWatcher
    │
    ├─→ Phase 3 (Session + TUI 交互)  ─── 依赖 Phase 1 的 TUI + API 基础
    │         └─── 也依赖 Phase 2 的 command 模块
    │
    └─→ Phase 4 (打磨)  ─── 依赖所有前序阶段完成
```

### 并行化建议

- Phase 2 与 Phase 3 可以并行推进（命令扩展 vs session + TUI 交互属不同模块）
- Phase 3 中 TUI 部分（slash 指令 / model 面板）与 session 部分也可并行
- Phase 4 必须串行（作为最终打磨阶段）

---

## 目录 → 阶段 速查表

| 目录 | Phase 1 | Phase 2 | Phase 3 | Phase 4 |
|------|:---:|:---:|:---:|:---:|
| `src/config/` | ★ | | | |
| `src/api/` | ★ | ☆ | | |
| `src/tui/` | ★ | ★ | ★ | |
| `src/command/` | ★ | ★ | ★ | ★ |
| `src/session/` | | ★ | | |
| `src/prompt/` | ★ | ☆ | | |

> ★ = 主要开发阶段，☆ = 增量完善，空格 = 不涉及
