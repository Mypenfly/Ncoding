# src/config/ — 配置加载

N-coding 的 KDL 格式配置文件解析与优先级合并（工作目录 > 全局目录 > 程序默认值）。

## 设计概要

配置文件采用 KDL（KDL Document Language）格式，支持：
- **工作目录级**：`./n_coding.kdl`（项目专属）
- **用户全局**：`~/.config/ncoding/config.kdl`（跨项目默认）
- **环境变量**：如 `DEEPSEEK_API_KEY`（最高优先级覆盖）

高优先级覆盖低优先级，未配置项使用程序内置默认值。

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块入口，re-export 配置类型 |
| `loader.rs` | KDL 配置解析 + 三级优先级合并 + 默认值填充 |

## 配置节

| 节 | 说明 |
|------|------|
| `api` | 模型、endpoint、api_key、token 限制、温度等 |
| `thinking` | thinking mode 开关 + reasoning_effort 级别 (high/max) |
| `session` | sessions_dir、backups_dir、max_context_messages、verbose |
| `skills` | skills 本地目录、auto_load_list |
| `tools` | 外部工具定义列表（name → description + exec） |
| `safety` | deny_patterns 数、shell_timeout、file_max_read_lines |
| `character` | 可选的角色设定 prompt |

## 配置加载优先级

```
环境变量 (DEEPSEEK_API_KEY)
    ↓ 覆盖
工作目录 n_coding.kdl
    ↓ 覆盖
~/.config/ncoding/config.kdl
    ↓ 覆盖
程序内置默认值
```

## 对应阶段

- **Phase 1**：核心 KDL 解析 + 优先级合并

## 参考

- 设计文档：`docs/ncoding.md` 第 9 节（配置文件）
