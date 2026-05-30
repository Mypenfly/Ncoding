//! System prompt builder — assembles the full system prompt from
//! character config, command grammars, shell info, and tool definitions.
#![allow(dead_code)]

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
            self.build_time_injection(),
            self.command_grammar_prompt(),
            self.shell_prompt(),
            self.files_operator_prompt(),
            self.sub_agent_task_prompt(),
            self.agent_skills_prompt(),
            self.checklist_prompt(),
            self.agent_logs_prompt(),
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

    fn build_time_injection(&self) -> String {
        format!(
            "当前时间: {} (UTC). 距离你的训练数据截止日期已过了一段时间，如有不确定的信息请使用工具查询。",
            chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
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
- 命令块建议使用截止符 <<<[__END__]>>> 结束，如果不使用下一个命令开始或输出结束也会自动结束，但这时可能出现解析问题"#
            .into()
    }

    fn shell_prompt(&self) -> String {
        r#"## Shell Command

你可以使用 Shell 命令在终端中执行操作。
你的 shell 环境是 nushell (nu)，不是 bash。请使用 nushell 语法编写命令。
系统已安装以下额外工具：rg (ripgrep), jj (jujutsu)。

重要提示：
- 使用 help commands 查看所有可用命令
- 使用 help <command> 查看具体命令的用法
- 文件搜索请使用 rg (ripgrep) 而非 find，get 命令
- 目录搜索请使用 ls **/* | where ...
- 长运行时间的命令（如 cargo build）建议使用 is_async=true

语法：
<<<[Shell]>>>
「command」:「「你的命令」」
「is_async」:「「true 或 false」」
<<<[__END__]>>>

参数说明：
- command: 要执行的 nushell 命令（必填）
- is_async: 是否异步执行（选填，默认 false）
  - false: 同步执行，等待结果返回（默认超时120秒）
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

    fn checklist_prompt(&self) -> String {
        r#"## CheckList Command

规划和管理任务列表。当进行复杂的多步骤工作时，优先使用此命令创建和跟踪任务。

### Create — 创建新任务
<<<[CheckList]>>>
「mode」:「「create」」
「title」:「「修复登录超时问题」」
「content」:「「调查 auth_service.rs 中 30 秒超时的原因，检查数据库连接池配置」」
<<<[__END__]>>>

### Update — 更新任务状态
<<<[CheckList]>>>
「mode」:「「update」」
「id」:「「任务的ID」」
「status」:「「in_progress」」
<<<[__END__]>>>

支持的状态: waiting, in_progress, done, failed, cancelled

### List — 列出所有任务
<<<[CheckList]>>>
「mode」:「「list」」
<<<[__END__]>>>

重要规则：
- 启动复杂任务时先创建 CheckList 规划步骤
- 开始执行时更新为 in_progress，完成时更新为 done
- 系统会根据未完成任务自动提醒你继续工作
- 使用 list 模式查看当前进度"#
            .into()
    }

    fn agent_logs_prompt(&self) -> String {
        r#"## AgentLogs Command

读写开发日志，用于记录关键决策、踩坑经验、架构变更等。日志保存在 .ncoding/agent_logs/ 中。

### Write — 写入日志
<<<[AgentLogs]>>>
「mode」:「「write」」
「filename」:「「fix_auth_timeout.md」」
「content」:「「根因：数据库连接池 max_connections=5 不够导致排队超时。解决方法：增大到 20，并添加连接健康检查。」」
<<<[__END__]>>>

### Read — 读取日志
<<<[AgentLogs]>>>
「mode」:「「read」」
「filename」:「「fix_auth_timeout.md」」
<<<[__END__]>>>

### List — 列出所有日志
<<<[AgentLogs]>>>
「mode」:「「list」」
<<<[__END__]>>>

推荐用法：
- 解决完重要 bug 后写日志记录原因和方法
- 做架构决策时写日志记录 trade-off 分析
- 文件名建议用描述性的 slug（如 fix_auth_timeout.md），不写则自动生成时间戳文件名"#
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
