# src/command/ — 命令系统

N-coding 的核心机制：基于模型输出文本正则匹配的 Command System，不依赖原生 tool_call/function_call。

## 设计概要

模型流式输出 token 的同时，CommandWatcher 异步监听 buffer 中出现的命令语法块，解析到完整命令后立即 `tokio::spawn` 并发执行。命令结果通过系统软注入回传至下一轮 user 消息。

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 命令系统入口 + CommandWatcher（buffer 监听、解析调度、命令分发） |
| `parser.rs` | `<<<[Command]>>>` 正则解析器（命令名标准化、key/value 提取、`---` 分隔、`__END__` 截止） |
| `syntax.rs` | 核心数据结构 enum：`NCommand`, `ShellBlock`, `FileOpBlock`, `ToolCallBlock`, `SubAgentBlock`, `SkillsBlock`, `CommandResult` |
| `shell.rs` | Shell 命令执行（nushell `nu -c`、安全审查、sync/async 模式、超时） |
| `files_operator.rs` | FilesOperator（read 带行号返回 / write 创建覆盖 / edit 精确字符串替换+唯一性检查） |
| `tool_call.rs` | ToolCall 外部工具调用（查配置 → 执行外部程序 → 收集 stdout） |
| `sub_agent_task.rs` | SubAgentTask 子任务委派（独立上下文、禁止循环调用、返回最后一条输出） |
| `agent_skills.rs` | AgentSkills 技能系统（list / load，工作目录 + 全局目录双路径搜索） |

## 对应阶段

- **Phase 1**：`parser.rs`, `syntax.rs`, `shell.rs`（sync 模式）, `mod.rs`（CommandWatcher 基础）
- **Phase 2**：`files_operator.rs`, `tool_call.rs`, `sub_agent_task.rs`, `agent_skills.rs`
- **Phase 3**：`shell.rs`（async 模式 + 安全审查完善）
- **Phase 4**：错误处理完善

## 参考

- 设计文档：`docs/ncoding.md` 第 2 节（Command System 语法）、第 4 节（内置命令）、第 5 节（双向性）、第 6 节（系统软注入）
- 来源参考：`docs/narwhal_dev.md` 第 3 节（Command System）
