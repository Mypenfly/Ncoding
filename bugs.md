# FileOperator Edit 模式反复失败 Bug 报告

## 问题现象

在执行 `files_operator.rs` 优化任务时，多次使用 `FilesOperator.edit` 模式替换 `edit_file` 函数（约35行），系统反复返回 "ambiguous match" 错误，候选匹配数量极多（2 到 97 个），但实际文件中只有一个匹配位置。

错误示例：
```
edit failed: ambiguous match — 97 possible blocks in src/command/files_operator.rs.
Candidates: lines 287..323, lines 287..336, lines 287..337, ... lines 287..939, lines 287..940
```

文件总约 940 行，但所有候选均从行 287 开始，结束行从 323 一直到文件末尾，这显然不合理。

## 根因

`edit_lines` 函数（`src/command/files_operator.rs:409-520`）的匹配算法问题：

1. **首尾定位过于宽松**：算法提取 `old_lines` 的第一行和最后一行作为 anchor，在文件中分别找到所有匹配位置（`first_matches` / `last_matches`），然后对所有 `(first, last)` 组合进行中间行匹配。

2. **`match_intermediate` 在非候选区间返回 true**：该函数（第 717 行）在当前范围 `[start, end]` 内顺序扫描所有行，只要所有 anchored 行以顺序正确出现在区间内就算匹配。但问题是：
   - 对于 `start=287, end=323`（正确区间），anchored 行顺序匹配成功（true）
   - 对于 `start=287, end=940`（到文件末尾），anchored 行仍然顺序匹配成功（true），因为区间包含了整个文件的后续部分，自然包含了所有 anchored 行

   这导致几乎所有的 `(first, last)` 组合都被判定为 valid，因为只要 `last` 足够大能包含所有 anchored 行即可。

3. **缺少区间内严格边界检查**：`match_intermediate` 没有检查 anchored 行必须在区间的**末尾部分**匹配，也没有要求区间内不允许有额外的 anchored 行重复匹配。因此单个正确的区间被扩展成大量误匹配。

4. **`...` segment 分割也被影响**：`split_segments` 分出的 segments 在 `find_segment_positions` 中同样使用了宽松的匹配（`normalized_match` 忽略空白），但这里不是主因。

## 建议修复方向

- `match_intermediate` 应该确保**所有的锚行严格按顺序匹配，且每个锚行只匹配区间内的唯一位置**，不允许跳跃式匹配导致区间膨胀。
- 或者，改为先通过 segments 精确定位每个 segment 的起止行（利用 `find_segment_positions`），然后以 segment 的组合作为唯一区间候选，而不是先确定首尾再验证中间。
- 对于相同的首行和尾行匹配（如多段重复代码），应该要求所有 intermediary anchored lines 在区间内精确连续匹配，而不是只要"存在"即可。

## 影响

**阻塞编辑操作**：任何稍复杂的编辑请求（如替换一个函数）都可能因为算法过度匹配而失败，需要使用者反复尝试提供更独特的内容，但实质上算法的唯一匹配逻辑应当是可以工作的，问题出在候选的生成和过滤上。

## 环境

- 文件：`src/command/files_operator.rs` (约940行)
- 编辑目标：替换 `edit_file` 函数定义
- old_lines：精确复制了函数签名+函数体（包括缩进）
- 返回：ambiguous match，大量候选

---

## 修复后验证结果（2026-05-31）

### 通过的场景
| 测试 | 结果 |
|------|------|
| 458 行文件，50 个相似函数，用唯一 body 行 edit 特定函数 | ✅ 正确唯一匹配 |
| 5 个 `fn process_*` 函数，edit 特定函数 | ✅ 正确唯一匹配 |
| 3 个 struct impl，自然唯一匹配 | ✅ 正确唯一匹配 |

### 仍有残余的 ambiguous match 场景
| 测试 | 结果 |
|------|------|
| 10 个完全相同签名的函数（`pub fn process`），用唯一 body 行（`let x = 5;`）edit | ⚠️ 匹配 4 个 block，报歧义错误 |
| 同上场景，用 `...` 通配整个 body | ⚠️ 匹配 9 个 block，报歧义错误 |

**结论**: 原报告中候选数爆炸（97 个）的核心问题已大幅改善，但在高度重复代码块的极端场景下，算法仍会产生多个误匹配（约 4 个），尚未达到完全唯一匹配。建议继续按照根因分析中的方向进一步收紧匹配边界检查。
