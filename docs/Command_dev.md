# Command System 开发文档

本文档详细讲解 N-coding 命令系统的解析原理、执行流程，以及开发过程中遇到的关键问题和解决方案。

---

## 1. 命令语法

### 1.1 基本语法

```
《[CommandName]|character|》
【key_1】value_1【key_1】
【key_2】value_2【key_2】
《[End]|CommandName|》
```

**各部分说明：**

| 部分 | 含义 | 规则 |
|------|------|------|
| `《[CommandName]|` | 命令开始 + 命令名 | 大小写不敏感，解析时消除特殊字符全大写匹配 |
| `character` | agent 身份标识 | 主 agent 为 `Ncoder`，subagent 为 `Subcoder` |
| `|》` | 开始标记闭合 | 必须是 `\|》`（`|` + `》`），否则跳过 |
| `【key】value【key】` | 键值对 | key 必须对称闭合，value 可以是任意长文本（含换行） |
| `《[End]|CommandName|》` | 命令结束（**必需**） | CommandName 必须与开头一致，否则拒绝执行并报错 |

### 1.2 分隔符与注释

| 语法 | 用途 |
|------|------|
| `---` | 同一命令的多个参数块分隔。仅当 `---` 出现在键值对**外部**时才生效 |
| `/-...-/` | 行内注释，不解析为参数 |
| `//-...-//` | 块注释，包含的命令整体被跳过 |

---

## 2. 解析原理

### 2.1 整体流程

```
原始文本 buffer
    │
    ├─ 扫描 《[ 标记（字节级扫描，0xE3 0x80 0x8A）
    │
    ├─ 提取 CommandName 和 character
    │
    ├─ 查找匹配的 《[End]|CommandName|》（深度追踪）
    │
    ├─ 提取 body（两个标记之间的文本）
    │
    ├─ 去除 /-...-/ 行内注释
    │
    ├─ 解析键值对（栈式匹配）
    │
    └─ 构建 NCommand → 交给执行器
```

### 2.2 标记扫描（字符级，非正则）

命令起始标记 `《` 的 UTF-8 编码是 `E3 80 8A`，结束标记 `》` 是 `E3 80 8B`。键标记 `【` 是 `E3 90 90`，`】` 是 `E3 80 91`。

**为什么不用正则？** 正则在高频流式调用中性能较差，且无法优雅处理嵌套深度追踪。字节级扫描使用简单的 `find_byte` 线性搜索，稳定可靠。

`scan()` 函数的核心循环：

```
pos = 0
while pos < buffer.len():
    tag = find_tag(buffer, pos)    // 找 《
    检查 《[CommandName]|character|》 的完整性
    提取 CommandName 和 character
    body_start = 标记结束位置
    body_end = find_end_depth(buffer, body_start, cmd_name)  // 深度追踪
    提取 body → 解析键值对 → 构建命令 → 推进 pos
```

### 2.3 End 标记的深度追踪

这是解决**内容中包含命令语法**问题的关键机制。

**问题场景：** 模型在 FilesOperator 的 `【content】` 中写入包含命令语法的文本：

```
《[FilesOperator]|Ncoder|》
【content】以下是 Shell 命令示例：
  《[Shell]|Ncoder|》
  【command】ls【command】
  《[End]|Shell|》
以上是示例。【content】
《[End]|FilesOperator|》
```

如果简单搜索第一个 `《[End]|FilesOperator|》`，会在内容中的 `《[End]|Shell|》` 处错误终止。

**解决方案：** `find_end_depth()` 维护一个深度计数器：

```
depth = 0
扫描 body 中的每个 《[Command]|...|》 标记：
  如果标记名 == "End" 且 end_name == cmd_name：
    如果 depth == 0 → 这是真正的结束标记，返回其位置
    如果 depth > 0 → depth -= 1（匹配了一个嵌套的开始）
  如果标记名 != "End" 且 标记名 == cmd_name → depth += 1
```

这样，内容中嵌套的 `《[Shell]|Ncoder|》` 会使 depth += 1，而嵌套的 `《[End]|Shell|》` 会使 depth -= 1，只有 depth == 0 时的 `《[End]|FilesOperator|》` 才是真正的结束。

同样的逻辑也适用于递归嵌套（FilesOperator 内容里再嵌套 FilesOperator）。

### 2.4 键值对解析（栈式匹配）

键值对使用 `【([^】]+)】` 正则找到所有标记，然后用**栈**进行配对。

**数据结构：** `stack: Vec<(key_name, value_start, marker_start)>`

**算法：**

```
for each 【key】 marker (按顺序):
    if key 已经在栈中:
        // 这是闭合标记
        从栈中取出 key 之上的所有项（它们是被嵌套的、自动闭合的键）
        为每个自动闭合的键设置 value
        为 key 本身设置 value（从 value_start 到当前标记位置）
    else:
        // 这是新的开始标记
        push (key, value_start, marker_start)
```

**为什么用栈？** 键可以嵌套。例如：

```
【content】The command is:
  【command】echo hello【command】
End of example.【content】
```

这里的 `【command】` 嵌套在 `【content】` 内部。栈式解析：
1. push content
2. push command（嵌套）
3. pop command（闭合，value = "echo hello"）
4. pop content（闭合，value = "The command is: ... End of example."）

`【command】` 作为嵌套键被正确提取，不影响外层的 `【content】` 值。

### 2.5 `---` 分隔符的安全处理

**问题场景：** 模型写入 README.md 时，内容包含大量 Markdown 水平分隔线 `---`。旧代码在 `parse_body` 中无条件按 `---` 分割 body，导致一个完整的 key-value 被切成 31 个碎片。

**解决方案：** `split_body_by_separators()` 只在实际键值对**外部**的 `---` 处分割。

**流程：**

```
1. compute_kv_ranges(body) → 使用栈式计算所有键值对占据的字节范围
   例如: [(mode_start, mode_close), (path_start, path_close), (content_start, content_close)]

2. split_body_by_separators(body, kv_ranges) → 遍历所有 ---：
   - 如果 --- 在某个 kv_range 内 → 跳过（这是值的一部分）
   - 如果 --- 不在任何 kv_range 内 → 真实分隔符，分割 body

3. 对每个分割后的 section 分别调用 parse_section
```

这样，`【content】` 值内部的 31 个 `---` 全部被跳过，只产生一个 FilesOperator block。

---

## 3. 执行流

### 3.1 流式解析与即时执行

```
API SSE 流式响应
    │
    ├─ ContentChunk token → tui_rx
    │
    ├─ handle_tui_event(ContentChunk):
    │   ├─ 追加到 content_text（TUI 显示）
    │   └─ cmd_watcher.feed_token(token)
    │
    ├─ feed_token(token):
    │   ├─ 追加到内部 buffer
    │   └─ loop: extract_commands_from(buffer, last_scan, is_final=false)
    │       ├─ 如果找到完整命令（有匹配的 《[End]|Cmd|》）：
    │       │   ├─ tokio::spawn(execute_commands) → 异步执行
    │       │   └─ 记录到 pending_handles
    │       └─ 如果命令不完整（End 标记还未到达）：
    │           └─ 保留 last_scan，等待下一个 token
    │
    └─ StreamDone:
        ├─ 保存 assistant 消息
        ├─ 剥离 cmd_watcher → tokio::spawn 后台收集
        │   ├─ finalize() → 提取剩余命令
        │   └─ drain_results() → 等待所有 pending_handles
        └─ CommandsCompleted:
            ├─ format_command_results → <(<(SYSTEM...[Result]...)>)>
            ├─ push 到 messages（作为 user 注入）
            └─ continue_conversation() → 下一轮 API 请求
```

### 3.2 非阻塞设计

关键设计决策：**不在 TUI 事件循环中等待命令执行结果。**

旧代码在 `handle_stream_done` 中直接 `await drain_results()`，如果 Shell 命令需要 120 秒（超时），TUI 会冻结 120 秒。

新设计：

```
handle_stream_done:
    cmd_watcher 被 std::mem::take 移出
    tokio::spawn(async {
        drain_results().await
        通过 tui_tx 发送 CommandsCompleted 事件
    })
    // 立即返回，TUI 事件循环继续运行
```

### 3.3 用户直接输入命令

当用户在输入框中直接输入命令语法时（不以模型输出为中介）：

```
handle_user_input:
    if input.starts_with("《["):
        extract_commands_from_final(input) → 解析命令
        if 纯命令（剩余文本为空）:
            execute_commands → 显示结果 → return（不发 API）
        else:
            按正常用户输入处理（发给模型）
```

### 3.4 自动继续

当模型产生命令执行结果后，自动发起下一轮 API 请求以告知模型结果：

```
CommandsCompleted:
    if results 非空:
        format → push user 系统注入 → continue_conversation
    else if CheckList 有未完成任务:
        push 未完成提示 → continue_conversation
    else:
        设置 AppState::Stop（等待用户输入）
```

**自动继续无次数上限**，模型可以自主执行多轮命令直到任务完成。

---

## 4. 关键问题与解决

### 4.1 `---` 在 content 值中导致多 block 碎片

**现象：** 写入 README.md 时出现 3 个错误结果（"requires content parameter" × 2 + "Is a directory" × 1）

**根因：** 旧 `parse_body` 无条件按 `---` 分割 body。README 内容含 31 个 Markdown `---`，导致 32 个碎片 section，只有第一个 section 包含有效键值对，其余全是空 block 或路径缺失。

**解决：** 第 2.5 节所述的 `split_body_by_separators` + `compute_kv_ranges`。

### 4.2 内容中的命令语法导致边界错误

**现象：** FilesOperator 写入包含命令示例的文本时，`《[End]|FilesOperator|》` 被错误地匹配到内容中的 `《[End]|Shell|》`。

**根因：** 简单的字符串搜索找到第一个匹配的 `《[End]|FilesOperator|》`，但内容中可能存在嵌套的命令语法。

**解决：** 第 2.3 节所述的深度追踪 `find_end_depth`。

### 4.3 中流检查导致参数不完整

**现象（已修复）：** 模型输出 `《[ToolCall]|Ncoder|》` 然后 `「tool_name」:「「web_search」」` 时，旧代码立即构建并执行命令，但此时 args 为空（query、count 等后续参数还未到达）。

**根因：** `extract_commands_from` 在 body 到达 buffer 末尾时也提取命令（`body_end == buffer.len()`），导致未完整传输的参数被忽略。

**解决：** 中流模式 (`is_final=false`) 只在找到完整 End 标记时才提取命令。`is_final=true`（stream 结束）时允许无 End 标记提取。

### 4.4 Shell 命令包含 `【is_async】` 文本泄漏

**现象：** 模型输出 `【command】ls【is_async】true【command】` 时，命令参数包含了 `【is_async】true` 作为命令文本的一部分。

**根因：** 旧键值解析从 `【command】` 匹配到下一个 `【command】` 作为值，中间的所有内容（包括 `【is_async】true`）都成了 command 的值。

**解决：** 栈式匹配（第 2.4 节）。当 `【is_async】` 出现在 `【command】` 内部且 `is_async != command` 时，`【is_async】` 被推入栈作为嵌套键。当 `【command】` 闭合时，`is_async` 被自动闭合，分别取值。最终 command="ls"（截断在 `【is_async】` 之前），is_async="true"。

---

## 5. 代码结构

| 文件 | 职责 |
|------|------|
| `src/command/syntax.rs` | 核心数据结构：NCommand 枚举、ShellBlock、FileOpBlock 等 7 种 block 类型、CommandResult |
| `src/command/parser.rs` | 命令解析引擎：字节级扫描、深度追踪、栈式键值匹配、注释剥离 |
| `src/command/mod.rs` | CommandWatcher（流式监听 + 中流提取）、命令调度、结果格式化 |
| `src/command/shell.rs` | Shell 执行器（tokio::process::Command + nu -c） |
| `src/command/files_operator.rs` | 文件操作：Read（行号）、Write（覆盖）、Edit（精确替换） |
| `src/command/tool_call.rs` | 外部工具：KDL 配置 → CLI-arg JSON 传参 |
| `src/command/checklist.rs` | 任务规划：.ncoding/checklist.json CRUD + 未完成任务自动继续 |
| `src/command/agent_logs.rs` | 开发日志：.ncoding/agent_logs/ 读写 |
| `src/command/sub_agent_task.rs` | 子 agent 任务委派 |
| `src/command/agent_skills.rs` | Skills 系统：list/load |
