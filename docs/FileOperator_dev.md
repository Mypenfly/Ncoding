# FileOperator Edit 模式实现文档

Edit 模式采用**行匹配 + `...` 省略**机制，参考 Aider 的匹配策略设计。

---

## 1. 参数

| 参数 | 说明 |
|------|------|
| `old_lines` | 文件中需要被替换的代码块，支持 `...` 省略 |
| `new_lines` | 替换后的代码块，同样支持 `...` 保留原行 |

`old_str`/`new_str` 保留为兼容模式，但新用法推荐 `old_lines`/`new_lines`。

---

## 2. `...` 省略机制

模型可写 `...` 跳过无关代码，减少输出 token。系统按 `...` 分割为 segments，逐级匹配和替换：

```
【old_lines】
fn example() {
        ...
        if old_condition:
          do_old()
        ...
        return result
      }
【old_lines】
```

segments: `[["fn example() {"], ["if old_condition:", "do_old()"], ["return result", "}"]]`

---

## 3. 匹配算法

### 3.1 预处理
- `old_lines` / `new_lines` 整体 trim（去首尾 `\n`）
- 按行分割，过滤掉 `...` 和空白行，得到 anchored lines
- 匹配比较时忽略**所有空格、缩进、空行差异**（纯字符对比）

### 3.2 首尾定位
取 anchored 的首行和尾行，在文件中分别查找所有匹配位置（`first_matches` / `last_matches`）。

### 3.3 候选筛选
对每个 `(first_pos, last_pos)` 对（`last >= first`）：
- 强制要求首 anchored 行在 `first_pos` 精确匹配（`match_intermediate` 的第一行检查）
- 在此区间内顺序匹配所有 anchored 行

### 3.4 歧义处理
- 1 个候选 → 唯一匹配，执行替换
- 0 个候选 → 返回错误 + 候选首尾行号
- 多个候选 → 返回歧义错误 + 所有候选区间，提示增加上下文

### 3.5 Segment 级定位
用 `find_segment_positions` 直接在文件中定位每个 segment 的起止行，跳过空 segment。

---

## 4. 替换算法

1. 保留匹配区间之前的行
2. 对每个 segment，替换对应 old segment 的行：
   - 取原文件中该行的首非空格位置（indent）
   - 将 new segment 的对应行去缩进后，加上原 indent
   - 多余的 new 行沿用 segment 首行的 indent
3. 保留 non-segment 的间隙行和匹配区间之后的行
4. 维护原文件的末尾换行状态

---

## 5. 验证

替换后重新读取文件，检查所有 `new_anchored` 行是否在文件中出现（任一位置）。不通过则返回 verification failed 错误。

---

## 6. 写入安全

- 先写临时文件 `.filename.{pid}.{timestamp}.tmp`，再 `fs::rename` 原子替换
- 覆盖前可选备份到 `.ncoding/backups/<session>/`
- 写入后重新读取比对（write 模式），不匹配则报告错误

---

## 7. Write 模式

- 记录写入前 `path.exists()`，写入后用此值判断 `created` / `overwritten`
- 写入前 `create_dir_all(parent)` 自动创建父目录
- 上限 `MAX_WRITE_SIZE = 10MB`
- 写入后验证：重新读取比对，不一致则返回错误
