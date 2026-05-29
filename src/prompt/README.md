# src/prompt/ — 提示词构建

系统提示词的模板组装模块，将各命令的用法说明、角色设定、shell 环境信息拼接为完整 system prompt。

## 设计概要

系统提示词由多个模板模块拼接而成（`{{xxx}}` 为运行时替换的占位符）。每次 API 请求动态注入 `cur_time` 和已有命令执行结果到 user 消息中。

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块入口，re-export `PromptBuilder` |
| `builder.rs` | 提示词组装器：character_prompt + command_grammar + shell_prompt + files_operator_prompt + sub_agent_task_prompt + agent_skills_prompt + tool_call_prompt + tools_prompts |

## 提示词结构

```
{{character_prompt}}          — 角色设定（用户自定义或默认）
{{command_grammar}}           — 命令语法总览 + 系统软注入说明
{{shell_prompt}}              — Shell 环境告知（nushell / 已安装工具）
{{files_operator_prompt}}     — FilesOperator 用法和约束（强调先 read 后 edit）
{{sub_agent_task_prompt}}     — SubAgentTask 用法和约束
{{agent_skills_prompt}}       — Skills 系统用法
{{tool_call_prompt}}          — ToolCall 用法简表
{{tools_prompts}}             — 外部工具 name + description（从配置生成）
```

## 运行时动态注入

每次 user 消息前自动拼接：
```
<(<(SYSTEM
cur_time: 2026-05-29T14:30:00+08:00
[ShellResult] / [FileResult] / [SubAgentResult] / [ToolResponse]
)>)>
```

## 对应阶段

- **Phase 2**：完整的提示词模板体系
- **Phase 4**：Shell 环境信息注入完善

## 参考

- 设计文档：`docs/ncoding.md` 第 4 节（内置命令）中的提示词告知部分
- 提示词示例：`docs/ncoding_prompt_example.md`
