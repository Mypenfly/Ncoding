# src/tui/ — TUI 界面

基于 ratatui + crossterm 的终端交互界面，Everforest 暗色主题。

## 设计概要

极简 TUI 设计：无限滚动文本区 + 底栏自适应输入框 + 状态栏。支持 markdown 流式渲染、命令块折叠展开、slash 指令、快捷键操作。

## 布局（从上到下）

```
┌─ 标题栏 (1行): N-coding · model · session ─┐
│ 文本区域 (剩余高度): 无限滚动，显示对话        │
│   > 用户输入 (青色), 模型输出 (流式渲染)        │
│   <<<[Command]>>> (紫色边框), 返回结果 (绿色)  │
│─────────────────────────────────────────────│
│ Token 信息栏 (1行): In/Out/Cache/Cost        │
│─────────────────────────────────────────────│
│ 输入框 (1~5行自适应): > _                     │
│─────────────────────────────────────────────│
│ 状态栏 (1行): ~/workspace · model · thinking │
└─────────────────────────────────────────────┘
```

## 文件说明

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块入口，re-export `App`, `init_terminal`, `restore_terminal` |
| `app.rs` | TUI 应用状态机：事件循环、渲染调度、Slash 指令路由、API 调用触发 |
| `render.rs` | Markdown 流式渲染（代码块/标题/列表/内联代码）、命令块折叠样式 |
| `input.rs` | 输入框状态管理（文本/光标/Slash 检测） |
| `model_panel.rs` | /model 切换面板（模型列表 + thinking 开关 + reasoning_effort） |
| `theme.rs` | Everforest 配色方案常量 |

## 快捷键

| 按键 | 作用 |
|------|------|
| Enter | 发送输入 |
| Shift+Enter | 插入换行 |
| Ctrl+C | 退出 |
| Esc | 取消当前模型输出 |
| Ctrl+U | 清空输入框 |
| Tab | 展开/折叠命令块 |
| ↑/↓ | 滚动 |
| PgUp/PgDn | 翻页 |

## Slash 指令

`/help` `/clear` `/undo` `/model` `/model list` `/model <id>` `/model thinking on|off` `/session list|switch|rename|delete|current` `/skills` `/quit` `/cancel`

## Everforest 配色

| 元素 | 颜色 | Hex |
|------|------|-----|
| 用户输入 | blue | #7fbbb3 |
| 模型输出 | fg | #d3c6aa |
| 思考链 | grey2 | #939f91 |
| 命令块 | purple | #d699b6 |
| 命令结果 | aqua | #83c092 |
| 系统提示 | yellow | #dbbc7f |
| 错误 | red | #e67e80 |
| 状态栏 | orange | #e69875 |

## 对应阶段

- **Phase 1**：`app.rs`（基础事件循环）, `render.rs`（基础文本渲染）, `theme.rs`（配色）, `input.rs`（基础输入）
- **Phase 3**：`input.rs`（slash 指令）, `model_panel.rs`（/model 面板）
- **Phase 4**：`render.rs`（markdown 渲染完善 + 折叠展开交互）

## 参考

- 设计文档：`docs/ncoding.md` 第 7 节（TUI 设计）
