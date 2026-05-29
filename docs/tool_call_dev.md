# ToolCall 外部工具开发指南

ToolCall 是 N-coding 的外部工具调用系统，允许模型通过 `<<<[ToolCall]>>>` 语法调用用户自定义的外部程序/脚本。

## 1. 架构概览

```
模型输出 <<<[ToolCall]>>> → 解析参数块 → 查找工具定义 → 执行外部程序 → 收集输出 → 注入下一轮对话
```

- 工具在 KDL 配置文件中定义（name + description + exec 命令）
- 模型通过 ToolCall 命令调用，以 key-value 格式传递任意参数
- 所有参数序列化为 **JSON 字符串**，作为 CLI 参数传给工具
- 工具的 stdout/stderr 被收集并返回给模型
- **支持并行调用**：每次调用是独立进程，不同的 CLI 参数，不会相互干扰

## 2. 工具定义

在 `.ncoding/n_coding.kdl`（项目级）或 `~/.config/ncoding/config.kdl`（全局）的 `tools` 节中定义：

```kdl
tools {
    web_search description="搜索互联网信息" exec="python3" exec=".ncoding/tools/web_search.py"
    my_tool description="我的自定义工具" exec="bash" exec=".ncoding/tools/my_script.sh"
}
```

### 字段说明

| 字段 | 必填 | 说明 |
|------|------|------|
| 节点名 (tool_name) | 是 | 工具名称（KDL child node 的 name），模型用此名调用 |
| `description="..."` | 推荐 | 工具描述，会出现在系统提示词中帮助模型理解 |
| `exec="..."` | 是 | 执行命令。**每段一个 `exec=`**：第一个是命令本身，后续是固定参数。**注意此参数不是注入 args JSON 的那个，args JSON 是额外 append 到末尾的** |

### 配置示例

```kdl
# 简单命令: python3 script.py
tool1 description="desc" exec="python3" exec="script.py"

# 带 shell: bash script.sh
tool2 description="desc" exec="bash" exec="/path/to/script.sh"

# 带固定参数 (args JSON 追加在末尾)
tool3 description="desc" exec="python3" exec="tool.py" exec="--verbose"
```

## 3. 模型调用格式

```
<<<[ToolCall]>>>
「tool_name」:「「web_search」」
「query」:「「rust programming best practices」」
「count」:「「10」」
「lang」:「「zh」」
<<<[__END__]>>>
```

- `tool_name` — 必填，对应配置中的工具名
- 其他所有 key — 任意参数，都会被序列化后传给工具
- `---` 分隔符可调用多个工具（并发执行）

## 4. 参数传递（CLI 参数）

### 核心机制

所有 key-value 参数序列化为 JSON 字符串，作为**最后一个 CLI 参数**传递给工具程序：

```
实际执行:    exec_cmd exec_arg1 exec_arg2 ... "{\"tool_name\":\"web_search\",\"query\":\"rust tutorial\",...}"
```

JSON 结构：
```json
{
    "tool_name": "web_search",
    "query": "rust tutorial",
    "count": "5",
    "lang": "zh"
}
```

注：`tool_name` 也会自动包含在 JSON 中（可从 `block.args` 获取）。

### 为什么用 CLI 参数而非环境变量？

CLI 参数天然支持**并行调用**：多个工具并发执行时，每个进程有独立的 `argv`，不会像环境变量那样互相覆盖。

## 5. 工具程序示例

### Python（推荐）

```python
#!/usr/bin/env python3
import json
import sys

if len(sys.argv) < 2:
    print("Error: expected JSON args as argument", file=sys.stderr)
    sys.exit(1)

# 从第一个 CLI 参数读取 JSON
args = json.loads(sys.argv[1])

query = args.get("query", "")
count = args.get("count", "10")
lang = args.get("lang", "en")

# 执行实际逻辑...
result = f"Searching '{query}' in {lang}, count={count}"

# 输出到 stdout（此内容会返回给模型）
print(result)
```

### Shell

```bash
#!/bin/sh
if [ $# -lt 1 ]; then
    echo "Error: expected JSON args" >&2
    exit 1
fi

# 解析 JSON 参数
QUERY=$(echo "$1" | python3 -c "import sys,json; print(json.load(sys.stdin).get('query',''))")
echo "Processing query: $QUERY"
```

### Rust

```rust
use std::env;
use serde_json::Value;

fn main() {
    let args: Vec<String> = env::args().collect();
    let params: Value = serde_json::from_str(&args[1]).unwrap();
    let query = params["query"].as_str().unwrap_or("");
    println!("Query: {}", query);
}
```

## 6. 返回值

- **stdout** → 作为 `[ToolCallResult]` 的输出返回给模型
- **stderr** → 也包含在结果摘要中
- **exit_code** → 0 表示成功，非 0 表示失败
- **输出截断**：超过 100 行自动截断（保留前 50 + 后 50 行 + 概要）

## 7. 测试工具

1. 在 `.ncoding/n_coding.kdl` 中定义工具
2. 启动 N-coding
3. 输入测试指令：
```
/clear
请使用 ToolCall 调用 web_search 工具，查询关键词为 "rust async"，count 设为 3
```

或直接输入模拟的命令文本：
```
/clear
<<<[ToolCall]>>>
「tool_name」:「「web_search」」
「query」:「「rust async」」
「count」:「「3」」
<<<[__END__]>>>
```

## 8. 调试

- 工具的 stderr 会显示在返回结果的 `stderr:` 部分
- 可在工具脚本中使用 `print(..., file=sys.stderr)` 输出调试信息
- 日志文件 `.ncoding/n-coding.log` 包含执行信息
- 执行失败时，错误详情（包括 stderr）会在结果中返回

## 9. 安全注意

- 工具以当前用户权限执行
- 所有参数以 JSON 格式传递
- Shell 命令安全审查不适用于 ToolCall（工具由用户定义，视为可信）

## 10. 并行调用验证

以下场景展示了 CLI 参数传递的关键优势：

模型调用（三个并发工具）：
```
<<<[ToolCall]>>>
「tool_name」:「「my_tool」」
「marker」:「「AAA」」
---
「tool_name」:「「my_tool」」
「marker」:「「BBB」」
---
「tool_name」:「「my_tool」」
「marker」:「「CCC」」
<<<[__END__]>>>
```

实际执行的命令（三个进程同时启动，互不干扰）：
```sh
python3 tool.py "{\"tool_name\":\"my_tool\",\"marker\":\"AAA\"}"
python3 tool.py "{\"tool_name\":\"my_tool\",\"marker\":\"BBB\"}"
python3 tool.py "{\"tool_name\":\"my_tool\",\"marker\":\"CCC\"}"
```

## 11. 完整配置示例

```kdl
// .ncoding/n_coding.kdl

tools {
    web_search description="搜索互联网信息，参数: query, count, lang" exec="python3" exec=".ncoding/tools/web_search.py"
    code_review description="代码审查工具，参数: path, level" exec="bash" exec=".ncoding/tools/review.sh"
    fetch_url description="获取 URL 内容，参数: url, timeout" exec="python3" exec=".ncoding/tools/fetch.py" exec="--json"
}
```

第三个例子 `fetch_url`：`exec="--json"` 是传给 Python 脚本的固定参数，args JSON 会追加在 `--json` 之后。
此时实际执行为：`python3 .ncoding/tools/fetch.py --json "{\"url\":\"...\",\"timeout\":\"30\"}"`
