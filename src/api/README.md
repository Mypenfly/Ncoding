# src/api/ — DeepSeek API 客户端

封装 DeepSeek API 通信，支持 SSE 流式响应、reasoning_content/thinking mode 控制、模型列表获取和 session 命名。

## 设计概要

N-coding **不使用原生的 tool_call/function_call**。所有 API 调用为纯文本流式对话，命令解析由 command 模块完成。启用 `stream: true` 的 SSE 流式模式。

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块入口，re-export `DeepSeekClient` |
| `client.rs` | DeepSeek API 客户端：SSE 流式解析、reasoning_content 与 content 分离、usage 统计、模型列表、session 命名 |

## API 端点

- **Chat Completions**：`POST https://api.deepseek.com/chat/completions`（OpenAI 兼容格式）
- **Models List**：`GET https://api.deepseek.com/models`

## 核心类型

```rust
struct DeepSeekClient { base_url, api_key, model, http }
struct ChatRequest { model, messages, stream, thinking, max_tokens, temperature, top_p }
struct StreamChunk { choices: Vec<StreamChoice>, usage: Option<UsageInfo> }
enum TuiEvent { ReasoningChunk(String), ContentChunk(String), StreamDone, Error(String) }
```

## 关键设计决策

- **reasoning_content 保留**：assistant 消息同时存储 `reasoning_content` 和 `content`，下一轮 API 请求需同时携带（DeepSeek 不会自动补）
- **超长截断**：reasoning_content > 32K tokens 时保留前 16K + 后 8K
- **stream_chat 双通道输出**：`tui_tx`（TUI 渲染）+ `cmd_tx`（CommandWatcher 解析）
- 废弃参数不传：`frequency_penalty` / `presence_penalty`

## 定价参考（单位 $/1M tokens）

| 模型 | Input (cache miss) | Input (cache hit) | Output |
|------|-------------------|-------------------|--------|
| deepseek-v4-pro | $0.435 | $0.003625 | $0.87 |
| deepseek-v4-flash | $0.14 | $0.0028 | $0.28 |

## 对应阶段

- **Phase 1**：核心 SSE 流式客户端
- **Phase 3**：`list_models()` 实时模型列表、`generate_session_name()` session 命名

## 参考

- 设计文档：`docs/ncoding.md` 第 8 节（DeepSeek API 集成）
