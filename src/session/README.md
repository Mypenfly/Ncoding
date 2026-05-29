# src/session/ — Session 管理

基于工作目录 `.ncoding/sessions/` 的会话持久化和上下文管理，不实现复杂的 flet 筛选机制。

## 设计概要

每个 session 以 JSON 文件独立保存（路径：`.ncoding/sessions/<session_name>.json`），包含完整的 OpenAI 兼容格式 message 列表（含 reasoning_content）。会话名称由首次用户输入经 deepseek-v4-flash API 自动生成为 slug。

备份目录 `.ncoding/backups/<session_name>/` 用于存储 `/undo` 撤回所需的原始文件副本。

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块入口，re-export `SessionManager` |
| `manager.rs` | Session CRUD（创建/读/写/切换/命名/删除/列表）、消息追加与撤轮、上下文截断策略（保留最近 N 条） |
| `backup.rs` | 文件备份与恢复（`/undo` 撤回机制），FilesOperator 执行前自动备份原文件 |

## 核心数据结构

```rust
struct Session {
    name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    messages: Vec<Message>,  // OpenAI 兼容格式
}

struct Message {
    role: Role,               // System / User / Assistant
    reasoning_content: Option<String>,
    content: Option<String>,
}
```

## 关键设计决策

- **Session 命名**：首条用户输入 → 无上下文 API 请求（flash 模型） → 英文 slug → 同名追加数字后缀
- **上下文截断**：从旧到新保留最近 `max_context_messages`（默认 50）条完整消息，system prompt 始终保留
- **/undo 机制**：只撤回最近一轮，恢复文件 + 移除 message，Shell 副作用不自动撤回

## 对应阶段

- **Phase 3**：全部实现（session 读写、自动命名、切换、/undo、上下文截断）

## 参考

- 设计文档：`docs/ncoding.md` 第 3 节（Session 管理）、第 7.6 节（/undo 撤回机制）
