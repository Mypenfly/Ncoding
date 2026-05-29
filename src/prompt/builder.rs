#![allow(dead_code, unused_imports)]

//! System prompt builder — assembles the full system prompt from
//! character config, command grammars, shell info, and tool definitions.

use crate::config::loader::AppConfig;

pub struct PromptBuilder {
    config: AppConfig,
    shell_env: ShellEnvInfo,
}

pub struct ShellEnvInfo {
    pub os: String,
    pub shell: String,
    pub workspace: String,
    pub installed_tools: Vec<String>,
    pub available_skills: Vec<String>,
}

impl PromptBuilder {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            shell_env: ShellEnvInfo {
                os: std::env::consts::OS.to_string(),
                shell: "nushell".into(),
                workspace: std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| ".".into()),
                installed_tools: vec!["rg".into(), "jj".into()],
                available_skills: Vec::new(),
            },
        }
    }

    #[allow(clippy::useless_vec)]
    pub fn build(&self) -> String {
        let parts = vec![
            self.character_prompt(),
            self.command_grammar_prompt(),
            self.shell_prompt(),
            self.files_operator_prompt(),
            self.sub_agent_task_prompt(),
            self.agent_skills_prompt(),
            self.tool_call_prompt(),
            self.tools_prompts(),
        ];

        parts.join("\n\n")
    }

    pub fn build_env_injection(&self) -> String {
        format!(
            "<(<(SYSTEM\nos: {}\nshell: {}\nworkspace: {}\ninstalled_tools: {}\navailable_skills: {}\n)>)>",
            self.shell_env.os,
            self.shell_env.shell,
            self.shell_env.workspace,
            self.shell_env.installed_tools.join(", "),
            self.shell_env.available_skills.join(", ")
        )
    }

    fn character_prompt(&self) -> String {
        if let Some(ref ch) = self.config.character {
            ch.prompt.clone()
        } else {
            "你是一个专业的编程助手 N-coding，专长是编码、调试和软件设计。\n\
             你会使用 <<<[Command]>>> 语法调用工具来完成用户的任务。\n\
             在修改代码之前，你总是先阅读文件内容。\n\
             回答简洁，优先给代码而不是长篇解释。"
                .into()
        }
    }

    fn command_grammar_prompt(&self) -> String {
        r#"## Command System

你可以使用特殊的命令语法来调用工具。命令语法如下：

<<<[CommandName]>>>
「key1」:「「value1」」
「key2」:「「value2」」
<<<[__END__]>>>

- 命令名使用驼峰命名（如 ToolCall, Shell, FilesOperator），内部会自动忽略大小写和下划线
- 多个同类型命令用 --- 分隔
- value 中的内容可以包含换行和特殊字符，使用「「 」」包裹
- 命令块不是必须使用 <<<[__END__]>>> 结束，下一个命令开始或输出结束也会自动结束

## 系统软注入

你可能会在 user 消息中看到 <(<(SYSTEM ... )>)> 格式的内容，这是系统自动注入的信息，
用于告知当前时间、命令执行结果等。这是系统信息，不是你与用户对话的内容。请根据其中的信息来辅助决策。"#
            .into()
    }

    fn shell_prompt(&self) -> String {
        r#"## Shell Command

你可以使用 Shell 命令在终端中执行操作。
你的 shell 环境是 nushell (nu)，不是 bash。请使用 nushell 语法编写命令。
系统已安装以下额外工具：rg (ripgrep), jj (jujutsu)。

语法：
<<<[Shell]>>>
「command」:「「你的命令」」
「is_async」:「「true 或 false」」
<<<[__END__]>>>

参数说明：
- command: 要执行的 nushell 命令（必填）
- is_async: 是否异步执行（选填，默认 false）
  - false: 同步执行，等待结果返回
  - true: 异步执行，适用于 cargo build 等长命令

安全限制：
- 不允许使用 sudo 提权
- 不允许删除工作目录外的文件
- 不允许执行危险操作"#
            .into()
    }

    fn files_operator_prompt(&self) -> String {
        r#"## FilesOperator Command

用于读写和修改文件。支持三种模式：read、write、edit。

### Read — 读取文件
<<<[FilesOperator]>>>
「mode」:「「read」」
「path」:「「src/main.rs」」
「offset」:「「30」」
「limit」:「「80」」
<<<[__END__]>>>

### Write — 写入文件（创建或覆盖）
<<<[FilesOperator]>>>
「mode」:「「write」」
「path」:「「src/new_module.rs」」
「content」:「「文件内容」」
<<<[__END__]>>>

### Edit — 编辑文件（精确字符串替换）
编辑前你必须先使用 read 模式读取文件以获得真实内容！
<<<[FilesOperator]>>>
「mode」:「「edit」」
「path」:「「src/parser.rs」」
「old_str」:「「需要替换的文本」」
「new_str」:「「新文本」」
<<<[__END__]>>>

关键规则：
1. old_str 必须是文件中唯一出现的文本
2. old_str 的缩进、空行必须和文件中完全一致
3. 你必须先 read 文件获取真实内容，不能凭空构造 old_str
4. 尽量使用小范围精确编辑，不要用 edit 替换整个文件"#
            .into()
    }

    fn sub_agent_task_prompt(&self) -> String {
        r#"## SubAgentTask Command

你可以将子任务委派给另一个 agent 实例执行。subagent 使用相同的模型，独立上下文。
subagent 不能使用 SubAgentTask 命令。执行完毕后返回最后一条输出作为结果。

<<<[SubAgentTask]>>>
「prompt」:「「为 src/tui/render.rs 中的函数写测试」」
<<<[__END__]>>>
"#
        .into()
    }

    fn agent_skills_prompt(&self) -> String {
        r#"## AgentSkills Command

查看可用技能：
<<<[AgentSkills]>>>
「mode」:「「list」」
<<<[__END__]>>>

加载技能：
<<<[AgentSkills]>>>
「mode」:「「load」」
「skill_name」:「「test-driven-development」」
<<<[__END__]>>>

你可以使用 AgentSkills 命令加载更多能力。当遇到以下情况时请主动加载对应技能：
- 需要写测试 → test-driven-development
- 遇到 bug → systematic-debugging
- 任务完成需要审查 → code-review
使用「mode」:「「list」」查看当前可用的所有 skills。"#
            .into()
    }

    fn tool_call_prompt(&self) -> String {
        r#"## ToolCall Command

调用配置中定义的外部工具。

<<<[ToolCall]>>>
「tool_name」:「「web_search」」
「query」:「「rust error handling best practices」」
「count」:「「5」」
<<<[__END__]>>>
"#
        .into()
    }

    fn tools_prompts(&self) -> String {
        if self.config.tools.is_empty() {
            String::new()
        } else {
            let mut lines = vec!["可用的外部工具：".to_string()];
            for (name, def) in &self.config.tools {
                lines.push(format!("- {}: {}", name, def.description));
            }
            lines.join("\n")
        }
    }
}
