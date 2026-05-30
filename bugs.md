# FilesOperator Write/Edit 模式 Bug 分析

本文档列出 `src/command/files_operator.rs` 中 Write 和 Edit 模式的问题，供修复参考。

---

## 🔴 Bug 1 — Write 模式 action 永远显示 "overwritten"

**位置**: `files_operator.rs` 第 169-174 行

**根因**:
```rust
match fs::write(path, content) {
    Ok(()) => CommandOutcome::Success {
        summary: format!(
            "status: OK\npath: {}\naction: {}",
            path.display(),
            if path.exists() { "overwritten" } else { "created" }  // ← BUG
        ),
    },
```

`fs::write` 成功之后文件必然存在，因此 `path.exists()` 永远为 `true`，`action` 永远是 `"overwritten"`。首次创建文件时模型会误以为覆盖了已有文件。

**修复方向**: 在 `fs::write` **之前**记录是否存在，之后用它决定 action 文案。

**建议测试用例**:
- 创建新文件 → action 应为 `"created"`
- 覆盖已有文件 → action 应为 `"overwritten"`

---

## 🔴 Bug 2 — Edit 模式行号计算 off-by-one

**位置**: `files_operator.rs` 第 216 行

**根因**:
```rust
let start_line = file_content[..pos].lines().count() + 1;
```

`str::lines()` 的行为：如果切片以 `\n` 结尾，会多算一个空行。

**示例**: 文件 `"hello\nworld\nhello\n"`, old_str = `"world"`, pos = 6。
`file_content[..6]` = `"hello\n"` → `lines()` 返回 `["hello", ""]`，count = 2。
`start_line = 3`，但实际是第 2 行。

**触发条件**: `old_str` 之前的内容恰好以 `\n` 结尾时触发。

**修复方向**: 用字节计数替代 `lines()`:
```rust
let start_line = file_content[..pos].bytes().filter(|&b| b == b'\n').count() + 1;
```

**建议测试用例**:
- `"hello\nworld\nhello\n"` 中替换 `"world"` → 应报告第 2 行
- `"a\nb\nc\n"` 中替换 `"c"` → 应报告第 3 行
- `"a\nb\nc"` (无末尾换行) 中替换 `"c"` → 应报告第 3 行

---

## 🟡 Bug 3 — 空 old_str 导致灾难性错误消息

**位置**: `files_operator.rs` 第 200 行

**根因**:
```rust
let matches: Vec<_> = file_content.match_indices(old_str).collect();
```

`"".match_indices("")` 会在文件的每个字节位置产生匹配，包括字符串开头和结尾。对于 1000 行的文件，会返回几千个匹配，导致：
1. 错误消息包含几千行号，几乎不可读
2. 可能撑爆系统注入的 token 预算

**修复方向**: 在 `edit_file` 开头添加 guard:
```rust
if old_str.is_empty() {
    return CommandOutcome::Failure {
        error: "edit failed: old_str cannot be empty".into(),
    };
}
```

**建议测试用例**:
- `edit_file(..., Some(""), Some("x"))` → 应返回明确的空串错误，而不是几千行匹配

---

## 🟡 Bug 4 — Write 新文件到不存在的父目录会失败

**位置**: `files_operator.rs` 第 155-178 行

**根因**: `fs::write` 不会自动创建父目录。模型经常会写入不存在的目录下的新文件，例如：
- `.ncoding/agent_logs/fix.md` (目录尚未创建)
- `src/new_module/sub.rs` (子目录不存在)

**修复方向**: 在写入前调用 `fs::create_dir_all(parent)`：
```rust
if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|e| ...)?;
}
```

**建议测试用例**:
- 写入 `tmp_test/nonexistent_dir/new_file.txt` → 父目录自动创建，写入成功
- 写入已有目录下的新文件 → 正常创建

---

## 🟡 Bug 5 — 缺少备份机制，/undo 可能无法工作

**位置**: `files_operator.rs` 第 8-72 行 `execute` 函数

**根因**: 文档 §7.6 描述了 `/undo` 撤回机制，要求在 write/edit 之前将原始文件备份到 `.ncoding/backups/<session_name>/`。但 `execute` 函数中完全没有备份逻辑。

备份如果是在更上层（session manager）做，需要确认。如果没做则 `/undo` 形同虚设。

**修复方向**: 有以下两种方案：

**方案 A — 在 files_operator 里做备份（推荐）**:
在 `write_file` 和 `edit_file` 写入前，如果原文件存在，复制到 `.ncoding/backups/<session>/<timestamp>_<path>`。

**方案 B — 在 session manager 层做**:
在调用 `execute` 之前备份。但需要 session manager 能感知到哪些文件将被修改（需要提前解析 FileOpBlock），侵入性较高。

建议用方案 A，集中管理文件操作的安全性。需要考虑：
- 备份目录如何传入（当前 execute 不感知 session name）
- 备份文件命名（`{timestamp}_{path_with_underscores}`）
- 只有 write（覆盖）和 edit 需要备份，read 和 write（新建）不需要

**建议测试用例**:
- 覆盖已有文件 → 原内容被备份到 backups 目录
- edit 已有文件 → 原内容被备份
- 创建新文件 → 不产生备份
- 备份目录自动创建

---

## 🟢 Bug 6 — Write 没有内容大小限制

**位置**: `files_operator.rs` 第 155 行

**根因**: Read 模式有 2000 行限制，Write 模式对 content 大小无任何检查。如果模型异常输出（恶意或幻觉），可能写入超大文件。

**修复方向**: 添加可配置的大小上限（如 10MB），或在写入前检查 content.len()。

**优先级**: 低，实际触发概率小。

---

## 🟢 Bug 7 — 非原子写入，崩溃可能产生损坏文件

**位置**: `files_operator.rs` 第 167 行

**根因**: 直接 `fs::write(path, content)` 覆盖。如果程序在写入过程中崩溃或被 kill，文件会留下不完整的内容，且原始内容已丢失。

**修复方向**: 标准做法是写入临时文件 + 原子 rename：
```rust
let tmp = path.with_extension("tmp");
fs::write(&tmp, content)?;
fs::rename(&tmp, path)?;
```

**优先级**: 低，但实现成本也低，建议顺手修复。

---

## 修复优先级建议

| 优先级 | Bug | 修复工作量 |
|--------|-----|-----------|
| 🔴 高 | Bug 1 (action 永远 overwritten) | 1 行改动 |
| 🔴 高 | Bug 2 (行号 off-by-one) | 1 行改动 + 测试 |
| 🟡 中 | Bug 3 (空 old_str) | 3 行 guard |
| 🟡 中 | Bug 4 (父目录不创建) | 3-5 行 + 测试 |
| 🟡 中 | Bug 5 (无备份) | 20-40 行，需要传 session 信息 |
| 🟢 低 | Bug 6 (无写入上限) | 3 行 guard |
| 🟢 低 | Bug 7 (非原子写入) | 3-5 行改动 |

---

## 修复顺序建议

1. **先修 Bug 1 + Bug 2**（都是 1 行改动，立即见效，有明确测试用例）
2. **再修 Bug 3 + Bug 4**（简单 guard，防御性修复）
3. **Bug 7**（顺手改，3 行写完就没了，但注意和 Bug 5 的交互——如果在 Bug 5 中已经引入了 tmp 文件逻辑，可以合并）
4. **Bug 5**（工作量最大，需要决策：备份在哪个层做？需要传入 session name。建议先讨论方案再动手）
5. **Bug 6**（可选，极低概率）

## 其他注意事项

- 修复 Bug 4（创建父目录）后，Bug 1 的修复需要在这之后做（因为先 create_dir_all 不影响 exists 判断）。
- Bug 7（原子写入）如果用 tmp 文件，注意 tmp 文件名要避免冲突（加上进程 id 或随机后缀）。
- 所有修改完成后运行 `cargo test files_operator -- --nocapture` 确认通过。
