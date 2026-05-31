# FileOperator Edit 模式实现文档

Edit 模式采用**严格连续行匹配 + `...` 分段**机制。

---

## 1. 参数

| 参数 | 说明 |
|------|------|
| `old_lines` | 文件中需要被替换的代码块 |
| `new_lines` | 替换后的代码块 |

`old_str`/`new_str` 已废弃，不再支持。

---

## 2. 匹配算法

### 2.1 无 `...` — 连续匹配

`find_all_sequential_matches(file_lines, old_seg)`：从文件每个位置开始，逐行比较 `old_seg[j]` 和 `file_lines[start+j]`。使用 `normalized_match` 忽略所有空格和缩进差异（纯字符比较）。空行必须对应空行或纯空白行。

- 必须**严格连续**：old_seg 每行对应文件中的连续行
- **不允许跳过**文件中的额外行（如需跳过，使用 `...`）
- 每个起始位置最多产生 **1 个**候选

### 2.2 有 `...` — 分段匹配

`find_dotted_matches`：
1. `split_segments` 按 `...` 分行分割为 [head, ..., tail]
2. head segment 用 `find_all_sequential_matches` → heads
3. tail segment 用 `find_all_sequential_matches` → tails
4. 对每对 (head_match, tail_match)，验证中间 segments 在 head+1..tail-1 范围内顺序存在
5. 如果无候选但有唯一 head，用 tail end 作为宽松 fallback

### 2.3 歧义处理

- 1 个候选 → 唯一匹配
- 0 个候选 → 返回错误（首行找不到 / 无法连续匹配）
- 多个候选 → 返回错误 + **最小跨度候选的代码上下文**（前后 3 行），提示 "你可能想要修改此处"

---

## 3. 替换算法

### 3.1 无 `...` — `apply_simple_replace`

old_seg 有 M 行，new_seg 有 N 行，匹配区域 [start, end]：

- `min(M, N)` 行逐行替换，**保留原文件该行的缩进前缀**
- 如果 N > M（增量）：新增行追加在 end 后，首行缩进 = end 行的缩进，后续行缩进保持 new_lines 中的相对关系
- 如果 N < M（减量）：多出的 old 行直接删除
- 区域前后的文件内容完整保留

### 3.2 有 `...` — `apply_dotted_replace`

每个 segment 独立使用 `find_segment_in_range` 定位，segment 间间隙行保留原样。替换逻辑同 3.1。

---

## 4. 写入安全

- 先写临时文件 `.filename.{pid}.{timestamp}.tmp`，再 `fs::rename` 原子替换
- 覆盖前可选备份到 `.ncoding/backups/<session>/`

---

## 5. 验证

替换后用 `normalized_match` 检查所有 `new_anchored` 行是否在结果文件中出现。

---

## 6. Write 模式

- 上限 `MAX_WRITE_SIZE = 10MB`
- 写入前 `create_dir_all(parent)` 自动创建父目录
- 写入后重新读取比对
