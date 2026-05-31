use std::io;

use crossterm::{
    event::{
        self, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Layout},
    prelude::CrosstermBackend,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::mpsc;
use tracing::info;

use crate::{
    api::client::{DeepSeekClient, TuiEvent},
    command::parser::CommandParser,
    command::syntax::CommandResult,
    command::{execute_commands, format_command_results, CommandWatcher},
    config::loader::AppConfig,
    prompt::builder::PromptBuilder,
    session::manager::{Message, Role, SessionManager},
};

use super::input::InputState;
use super::theme::Theme;

pub type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Clone, Copy, PartialEq)]
enum AppState {
    Working,
    Stop,
}

pub struct App {
    config: AppConfig,
    api_client: DeepSeekClient,
    cmd_watcher: CommandWatcher,
    session_mgr: SessionManager,
    auto_continue_count: u32,
    state: AppState,
    input_state: InputState,
    scroll_offset: usize,
    auto_scroll: bool,
    last_draw_lines: std::cell::Cell<usize>,
    last_visible_h: std::cell::Cell<usize>,
    is_streaming: bool,
    thinking_text: String,
    content_text: String,
    token_info: String,
    status_text: String,
    tui_tx: mpsc::UnboundedSender<TuiEvent>,
    tui_rx: mpsc::UnboundedReceiver<TuiEvent>,
    name_generated: bool,
}

pub fn init_terminal() -> io::Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        )
    )?;
    Terminal::new(CrosstermBackend::new(stdout))
}

pub fn restore_terminal(terminal: &mut TuiTerminal) -> io::Result<()> {
    execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen
    )?;
    disable_raw_mode()?;
    terminal.show_cursor()
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let api_key = resolve_api_key(&config.api);

        let api_client = DeepSeekClient::new(
            config.api.base_url.clone(),
            api_key.clone(),
            config.api.model.clone(),
        );
        let prompt_builder = PromptBuilder::new(config.clone());
        let (tui_tx, tui_rx) = mpsc::unbounded_channel();

        let workspace = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into());

        let mut session_mgr = SessionManager::new(
            config.session.sessions_dir.clone().into(),
            config.session.backups_dir.clone().into(),
        );
        let _ = session_mgr.init_dirs();
        let session_name = format!("session-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
        session_mgr.prepare_session(&session_name);
        let system_prompt = prompt_builder.build();
        session_mgr.add_message_lazy(Message {
            role: Role::System,
            content: Some(system_prompt),
            reasoning_content: None,
        });

        let status_text = format!(
            "{} · {} · {} · thinking",
            workspace,
            config.api.model,
            session_mgr.current_name().unwrap_or("new"),
        );

        Self {
            api_client,
            cmd_watcher: CommandWatcher::new(),
            session_mgr,
            state: AppState::Stop,
            auto_continue_count: 0,
            input_state: InputState::new(),
            scroll_offset: 0,
            auto_scroll: true,
            last_draw_lines: std::cell::Cell::new(0),
            last_visible_h: std::cell::Cell::new(0),
            is_streaming: false,
            thinking_text: String::new(),
            content_text: String::new(),
            token_info: String::new(),
            status_text,
            tui_tx,
            tui_rx,
            config,
            name_generated: false,
        }
    }

    pub async fn run(mut self, terminal: &mut TuiTerminal) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(std::time::Duration::from_millis(10))? {
                if let Event::Key(key) = event::read()? {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    if self.is_streaming {
                        if key.code == KeyCode::Esc {
                            self.is_streaming = false;
                            self.content_text.push_str("\n\n[cancelled]");
                            self.handle_stream_done().await;
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Enter
                            if key.modifiers.contains(KeyModifiers::SHIFT)
                                || key.modifiers.contains(KeyModifiers::ALT)
                                || key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            self.input_state.insert_newline();
                        }
                        KeyCode::Enter if !self.input_state.is_empty() => {
                            let input_text = self.input_state.take();
                            if input_text.starts_with('/') {
                                self.handle_slash_command(input_text).await;
                            } else {
                                self.handle_user_input(input_text).await;
                            }
                        }
                        KeyCode::Char(c) => {
                            self.input_state.insert_char(c);
                        }
                        KeyCode::Backspace => {
                            self.input_state.delete_before_cursor();
                        }
                        KeyCode::Left => {
                            self.input_state.move_left();
                        }
                        KeyCode::Right => {
                            self.input_state.move_right();
                        }
                        KeyCode::Home => self.input_state.move_home(),
                        KeyCode::End => self.input_state.move_end(),
                        KeyCode::Up => {
                            let total = self.last_draw_lines.get();
                            let vis = self.last_visible_h.get();
                            if self.auto_scroll {
                                self.auto_scroll = false;
                                self.scroll_offset = total.saturating_sub(vis);
                            }
                            self.scroll_offset = self.scroll_offset.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            if self.auto_scroll {
                                continue;
                            }
                            let total = self.last_draw_lines.get();
                            let vis = self.last_visible_h.get();
                            self.scroll_offset =
                                (self.scroll_offset + 1).min(total.saturating_sub(1));
                            if self.scroll_offset >= total.saturating_sub(vis) {
                                self.auto_scroll = true;
                            }
                        }
                        KeyCode::PageUp => {
                            let total = self.last_draw_lines.get();
                            let vis = self.last_visible_h.get();
                            if self.auto_scroll {
                                self.auto_scroll = false;
                                self.scroll_offset = total.saturating_sub(vis);
                            }
                            self.scroll_offset = self.scroll_offset.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            if self.auto_scroll {
                                continue;
                            }
                            let total = self.last_draw_lines.get();
                            let vis = self.last_visible_h.get();
                            self.scroll_offset =
                                (self.scroll_offset + 10).min(total.saturating_sub(1));
                            if self.scroll_offset >= total.saturating_sub(vis) {
                                self.auto_scroll = true;
                            }
                        }
                        _ => {}
                    }
                }
            }

            while let Ok(event) = self.tui_rx.try_recv() {
                self.handle_tui_event(event).await;
            }
        }

        Ok(())
    }

    fn push_msg(&mut self, msg: Message) {
        self.session_mgr.add_message(msg).ok();
    }

    fn slash_reply(&mut self, text: String) {
        self.session_mgr
            .add_message(Message {
                role: Role::Info,
                content: Some(text),
                reasoning_content: None,
            })
            .ok();
    }

    fn msgs(&self) -> Vec<Message> {
        self.session_mgr.messages()
    }

    fn clear_non_system(&mut self) {
        self.session_mgr.truncate_context(1);
    }

    fn update_status(&mut self) {
        let workspace = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into());
        let state_str = match self.state {
            AppState::Working => "working",
            AppState::Stop => "stop",
        };
        self.status_text = format!(
            "{} · {} · {} · {}",
            workspace,
            self.config.api.model,
            self.session_mgr.current_name().unwrap_or("new"),
            state_str,
        );
    }

    async fn handle_user_input(&mut self, input_text: String) {
        if self.cmd_watcher.has_pending() {
            self.slash_reply("Commands still running, please wait...".into());
            return;
        }

        let trimmed = input_text.trim();
        if trimmed.starts_with("<<<[") {
            let mut parser = CommandParser::new();
            let (cmds, final_pos, _warnings) = parser.extract_commands_from_final(trimmed, 0);
            if !cmds.is_empty() && trimmed[final_pos..].trim().is_empty() {
                let results = execute_commands(cmds).await;
                let injection = format_command_results(&results);
                if !injection.is_empty() {
                    self.push_msg(Message {
                        role: Role::Assistant,
                        content: Some(injection),
                        reasoning_content: None,
                    });
                }
                return;
            }
        }

        self.push_msg(Message {
            role: Role::User,
            content: Some(input_text.clone()),
            reasoning_content: None,
        });

        self.auto_continue_count = 0;
        self.auto_scroll = true;

        // Trigger async session name generation on first user message
        if !self.name_generated {
            self.name_generated = true;
            self.session_mgr.activate().ok();
            let client = self.api_client.clone();
            let tx = self.tui_tx.clone();
            let user_text = input_text.clone();

            tokio::spawn(async move {
                if let Ok(name) = client.generate_session_name(&user_text).await {
                    let _ = tx.send(TuiEvent::SessionRenamed(name));
                }
            });
        }

        self.continue_conversation();
    }

    async fn handle_tui_event(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::ReasoningChunk(chunk) => {
                self.thinking_text.push_str(&chunk);
            }
            TuiEvent::ContentChunk(chunk) => {
                self.content_text.push_str(&chunk);
                self.cmd_watcher.feed_token(&chunk);
            }
            TuiEvent::StreamDone { usage } => {
                if let Some(u) = usage {
                    let cache_hit = u.prompt_cache_hit_tokens.unwrap_or(0);
                    let hit_pct = if u.prompt_tokens > 0 {
                        (cache_hit as f64 / u.prompt_tokens as f64) * 100.0
                    } else {
                        0.0
                    };
                    self.token_info = format!(
                        "Tokens · In: {}  Out: {}  Cache: {} (hit: {:.0}%)",
                        u.prompt_tokens, u.completion_tokens, cache_hit, hit_pct
                    );
                }
                self.handle_stream_done().await;
            }
            TuiEvent::Error(err) => {
                self.is_streaming = false;
                self.state = AppState::Stop;
                self.slash_reply(format!("[Error] {}", err));
            }
            TuiEvent::SessionRenamed(name) => {
                if let Err(e) = self.session_mgr.rename_current(&name) {
                    info!("Session rename failed: {}", e);
                } else {
                    self.update_status();
                }
            }
            TuiEvent::CommandsCompleted { results } => {
                self.handle_commands_completed(results).await;
            }
        }
    }

    async fn handle_stream_done(&mut self) {
        self.is_streaming = false;
        self.state = AppState::Working;

        let assistant_content = std::mem::take(&mut self.content_text);
        let reasoning = std::mem::take(&mut self.thinking_text);
        let reasoning_opt = if reasoning.is_empty() {
            None
        } else {
            Some(reasoning)
        };

        if !assistant_content.is_empty() {
            self.push_msg(Message {
                role: Role::Assistant,
                content: Some(assistant_content),
                reasoning_content: reasoning_opt,
            });
        }

        let (cmds, _errors) = self.cmd_watcher.finalize();
        let mut cmd_watcher = std::mem::take(&mut self.cmd_watcher);
        let tx = self.tui_tx.clone();
        tokio::spawn(async move {
            let mut results: Vec<CommandResult> = Vec::new();
            if !cmds.is_empty() {
                results.append(&mut execute_commands(cmds).await);
            }
            results.append(&mut cmd_watcher.drain_results().await);
            let _ = tx.send(TuiEvent::CommandsCompleted { results });
        });
    }

    async fn handle_commands_completed(&mut self, results: Vec<CommandResult>) {
        if !results.is_empty() {
            let injection = format_command_results(&results);
            if !injection.is_empty() {
                self.push_msg(Message {
                    role: Role::User,
                    content: Some(injection),
                    reasoning_content: None,
                });
                if true {
                    self.auto_continue_count += 1;
                    self.continue_conversation();
                }
            }
        } else {
            if crate::command::checklist::has_unfinished() {
                if let Some(summary) = crate::command::checklist::unfinished_summary() {
                    self.push_msg(Message {
                        role: Role::User,
                        content: Some(summary),
                        reasoning_content: None,
                    });
                    self.auto_continue_count += 1;
                    self.continue_conversation();
                } else {
                    self.state = AppState::Stop;
                }
            } else {
                self.state = AppState::Stop;
            }
        }
    }

    fn continue_conversation(&mut self) {
        let msg_count = self.msgs().len();
        info!(
            "API request: sending {} messages to model {}",
            msg_count, self.config.api.model
        );

        self.thinking_text.clear();
        self.content_text.clear();
        self.is_streaming = true;
        self.state = AppState::Working;
        self.cmd_watcher.clear();

        let client = self.api_client.clone();
        let tx = self.tui_tx.clone();
        let messages = self.msgs();
        let thinking_type = if self.config.thinking.enabled {
            "enabled"
        } else {
            "disabled"
        };
        let reasoning_effort = self.config.thinking.reasoning_effort.clone();
        let max_tokens = self.config.api.max_tokens;
        let temperature = self.config.api.temperature;
        let top_p = self.config.api.top_p;
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            let _result = client
                .stream_chat(
                    messages,
                    thinking_type,
                    &reasoning_effort,
                    max_tokens,
                    temperature,
                    top_p,
                    tx,
                    cmd_tx,
                )
                .await;
        });
    }

    async fn handle_slash_command(&mut self, input: String) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts.first().copied().unwrap_or("");

        match cmd {
            "/quit" | "/exit" | "/q" => {
                std::process::exit(0);
            }
            "/help" => {
                let help = "/help - show this help\n\
                /quit, /exit, /q - exit\n\
                /model - model panel (Phase 3)\n\
                /cancel - cancel current stream (Esc)\n\
                /clear - clear display\n\
                /session list - list all sessions\n\
                /session switch <name or id> - switch session\n\
                /session rename <name> - rename current\n\
                /session delete <name> - delete session\n\
                /session current - show current\n\
                /undo - undo last turn";
                self.push_msg(Message {
                    role: Role::Info,
                    content: Some(help.into()),
                    reasoning_content: None,
                });
            }
            "/clear" => {
                self.clear_non_system();
            }
            "/session" => {
                let sub = parts.get(1).copied().unwrap_or("help");
                self.handle_session_cmd(sub, &parts).await;
            }
            "/undo" => {
                let removed = self.session_mgr.remove_last_turn();
                match removed {
                    Some((_u, _a)) => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some("[undo] removed last turn".into()),
                            reasoning_content: None,
                        });
                    }
                    None => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some("[undo] nothing to undo".into()),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "/model" | "/skills" => {
                self.push_msg(Message {
                    role: Role::Info,
                    content: Some(format!("{}: coming in Phase 3", cmd)),
                    reasoning_content: None,
                });
            }
            _ => {
                self.push_msg(Message {
                    role: Role::Info,
                    content: Some(format!(
                        "unknown command: {}. Type /help for available commands.",
                        cmd
                    )),
                    reasoning_content: None,
                });
            }
        }
    }

    async fn handle_session_cmd(&mut self, sub: &str, parts: &[&str]) {
        match sub {
            "list" => match self.session_mgr.list_sessions() {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some("No sessions found.".into()),
                            reasoning_content: None,
                        });
                    } else {
                        let mut out = String::from("Sessions:\n");
                        for (i, (name, updated)) in sessions.iter().enumerate() {
                            let current_mark =
                                if Some(name.as_str()) == self.session_mgr.current_name() {
                                    " *"
                                } else {
                                    ""
                                };
                            out.push_str(&format!(
                                "  [{}] {} ({}){}\n",
                                i,
                                name,
                                updated.format("%Y-%m-%d %H:%M"),
                                current_mark
                            ));
                        }
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(out),
                            reasoning_content: None,
                        });
                    }
                }
                Err(e) => {
                    self.push_msg(Message {
                        role: Role::Info,
                        content: Some(format!("Error listing sessions: {}", e)),
                        reasoning_content: None,
                    });
                }
            },
            "switch" => {
                let name = parts.get(2).copied().unwrap_or("");
                if name.is_empty() {
                    self.push_msg(Message {
                        role: Role::Info,
                        content: Some("Usage: /session switch <name or id>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                // 支持数字索引切换
                let result = if let Ok(index) = name.parse::<usize>() {
                    self.session_mgr.load_session_by_index(index)
                } else {
                    self.session_mgr.load_session(name)
                };
                match result {
                    Ok(()) => {
                        self.update_status();
                        let msgs = self.msgs();
                        let display_name = self.session_mgr.current_name().unwrap_or(name);
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!(
                                "Switched to session '{}' ({} messages)",
                                display_name,
                                msgs.len()
                            )),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!("Failed to switch: {}", e)),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "rename" => {
                let new_name = parts.get(2).copied().unwrap_or("");
                if new_name.is_empty() {
                    self.push_msg(Message {
                        role: Role::Info,
                        content: Some("Usage: /session rename <new_name>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                match self.session_mgr.rename_session(new_name) {
                    Ok(()) => {
                        self.update_status();
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!("Renamed to '{}'", new_name)),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!("Rename failed: {}", e)),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "delete" => {
                let name = parts.get(2).copied().unwrap_or("");
                if name.is_empty() {
                    self.push_msg(Message {
                        role: Role::Info,
                        content: Some("Usage: /session delete <name>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                if Some(name) == self.session_mgr.current_name() {
                    self.push_msg(Message {
                        role: Role::Info,
                        content: Some("Cannot delete current session. Switch first.".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                match self.session_mgr.delete_session(name) {
                    Ok(()) => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!("Deleted session '{}'", name)),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Info,
                            content: Some(format!("Delete failed: {}", e)),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "current" => {
                let name = self.session_mgr.current_name().unwrap_or("none");
                let msgs = self.msgs();
                self.push_msg(Message {
                    role: Role::Info,
                    content: Some(format!(
                        "Current session: '{}' ({} messages)",
                        name,
                        msgs.len()
                    )),
                    reasoning_content: None,
                });
            }
            _ => {
                self.push_msg(Message {
                    role: Role::Info,
                    content: Some("Usage: /session [list|switch|rename|delete|current]".into()),
                    reasoning_content: None,
                });
            }
        }
    }

    fn draw(&self, f: &mut Frame) {
        let theme = Theme::everforest();
        let area = f.area();

        let input_vis = self
            .input_state
            .visual_lines(area.width.saturating_sub(4) as usize);
        let input_h = (input_vis
            .len()
            .max(1)
            .max(self.input_state.text.lines().count())
            .min(8)
            + 2) as u16;

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(input_h),
            Constraint::Length(1),
        ])
        .split(area);

        let title = format!(
            " N-coding · {} · {} ",
            self.config.api.model,
            if self.msgs().len() > 1 {
                "session"
            } else {
                "new"
            }
        );
        let title_block = Block::default().style(Style::default().fg(theme.bg));
        let title_span = Span::styled(title, Style::default().fg(theme.aqua));
        let title_line = Line::from(title_span);
        let title_p = Paragraph::new(title_line).block(title_block);
        f.render_widget(title_p, chunks[0]);

        let state_label = match self.state {
            AppState::Working => " WORKING ",
            AppState::Stop => " STOP ",
        };
        let state_style = match self.state {
            AppState::Working => Style::default().fg(theme.red).add_modifier(Modifier::BOLD),
            AppState::Stop => Style::default()
                .fg(theme.green)
                .add_modifier(Modifier::BOLD),
        };
        let state_line = Line::from(Span::styled(
            state_label,
            state_style.add_modifier(Modifier::BOLD),
        ));
        let state_width = state_label.len() as u16;
        let state_area = ratatui::layout::Rect {
            x: chunks[0].right().saturating_sub(state_width),
            y: chunks[0].y,
            width: state_width.min(chunks[0].width),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(state_line).block(Block::default()),
            state_area,
        );

        let text_area = chunks[1];
        let mut lines: Vec<Line> = Vec::new();

        const MAX_RENDER_LINES: usize = 5000;
        let display_msgs = self.msgs();
        for msg in &display_msgs {
            if lines.len() >= MAX_RENDER_LINES {
                break;
            }
            match msg.role {
                Role::System => {
                    lines.push(Line::from(Span::styled(
                        "[system prompt]",
                        Style::default().fg(theme.grey2),
                    )));
                }
                Role::User => {
                    let text = msg.content.as_deref().unwrap_or("");
                    if text.starts_with("【|Command/Tool|】")
                        || text.starts_with("(你调用的命令执行结果如下)")
                    {
                        for line_str in text.lines() {
                            lines.push(Line::from(Span::styled(
                                line_str,
                                Style::default().fg(theme.yellow),
                            )));
                        }
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("> ", Style::default().fg(theme.blue)),
                            Span::styled(text, Style::default().fg(theme.blue)),
                        ]));
                    }
                }
                Role::Assistant => {
                    if let Some(ref reasoning) = msg.reasoning_content {
                        if !reasoning.is_empty() {
                            for rline in reasoning.lines() {
                                if !rline.trim().is_empty() {
                                    lines.push(Line::from(Span::styled(
                                        rline,
                                        Style::default().fg(theme.grey2),
                                    )));
                                }
                            }
                        }
                    }
                    let text = msg.content.as_deref().unwrap_or("");
                    for line_str in text.lines() {
                        if lines.len() >= MAX_RENDER_LINES {
                            break;
                        }
                        lines.push(self.render_content_line(line_str, &theme));
                    }
                }
                Role::Info => {
                    let text = msg.content.as_deref().unwrap_or("");
                    for line_str in text.lines() {
                        lines.push(Line::from(Span::styled(
                            line_str,
                            Style::default().fg(theme.grey2),
                        )));
                    }
                }
            }
        }

        if self.is_streaming {
            if !self.thinking_text.is_empty() {
                for rline in self.thinking_text.lines() {
                    if !rline.trim().is_empty() {
                        lines.push(Line::from(Span::styled(
                            rline,
                            Style::default().fg(theme.grey2),
                        )));
                    }
                }
            }
            if !self.content_text.is_empty() {
                for line_str in self.content_text.lines() {
                    lines.push(self.render_content_line(line_str, &theme));
                }
            } else {
                lines.push(Line::from(Span::styled(
                    "...",
                    Style::default().fg(theme.grey2),
                )));
            }
        }

        self.last_draw_lines.set(lines.len());
        let visible_h = text_area.height.saturating_sub(1) as usize;
        self.last_visible_h.set(visible_h);

        let effective_offset = if self.auto_scroll {
            lines.len().saturating_sub(visible_h)
        } else {
            self.scroll_offset
                .min(lines.len().saturating_sub(visible_h))
        };

        let visible_lines: Vec<Line> = lines.into_iter().skip(effective_offset).collect();

        let text_widget = Paragraph::new(Text::from(visible_lines))
            .block(Block::default())
            .wrap(Wrap { trim: false });
        f.render_widget(text_widget, text_area);

        let token_bar = chunks[3];
        let token_text = if self.token_info.is_empty() {
            "Tokens · --"
        } else {
            &self.token_info
        };
        let token_line = Line::from(Span::styled(token_text, Style::default().fg(theme.orange)));
        f.render_widget(
            Paragraph::new(token_line).block(Block::default()),
            token_bar,
        );

        let input_area = chunks[4];
        let input_display = if self.input_state.text.is_empty() {
            "> _"
        } else {
            &self.input_state.text
        };
        let input_style = if self.is_streaming {
            Style::default().fg(theme.grey2)
        } else {
            Style::default().fg(theme.blue)
        };
        let input_widget = Paragraph::new(input_display)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme.aqua)),
            )
            .style(input_style)
            .wrap(Wrap { trim: false });
        f.render_widget(input_widget, input_area);

        if !self.is_streaming {
            let input_w = input_area.width.saturating_sub(2) as usize;
            let (cursor_row, cursor_col) = self.input_state.cursor_visual_position(input_w);
            let cursor_x = input_area.x + 1 + cursor_col as u16;
            let cursor_y =
                (input_area.y + 1 + cursor_row as u16).min(input_area.bottom().saturating_sub(2));
            f.set_cursor_position((cursor_x.min(input_area.right().saturating_sub(2)), cursor_y));
        }

        let status_bar = chunks[5];
        let status_line = Line::from(Span::styled(
            &self.status_text,
            Style::default().fg(theme.orange),
        ));
        f.render_widget(
            Paragraph::new(status_line).block(Block::default()),
            status_bar,
        );
    }

    fn render_content_line<'a>(&self, line: &'a str, theme: &Theme) -> Line<'a> {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            Line::from(Span::styled(line, Style::default().fg(theme.purple)))
        } else if trimmed.starts_with('#') {
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(theme.orange)
                    .add_modifier(Modifier::BOLD),
            ))
        } else if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(theme.aqua)
                    .add_modifier(Modifier::ITALIC),
            ))
        } else {
            Line::from(Span::styled(line, Style::default().fg(theme.fg)))
        }
    }
}

fn resolve_api_key(api: &crate::config::loader::ApiConfig) -> String {
    if !api.api_key.is_empty() {
        tracing::info!("Using api_key from config file");
        return api.api_key.clone();
    }

    let env_val = std::env::var(&api.api_key_env).unwrap_or_default();
    if !env_val.is_empty() {
        if env_val.starts_with("sk-") || env_val.starts_with("Bearer ") {
            tracing::warn!(
                "api_key_env '{}' contains what looks like an API key, not an env var name. \
                 Use 'api_key' in config instead of 'api_key_env' for direct key setting. \
                 Using the value as API key.",
                api.api_key_env
            );
            return env_val;
        }
        tracing::info!("Using API key from env var: {}", api.api_key_env);
        return env_val;
    }

    tracing::warn!(
        "No API key found. Set api_key in config or set {} env var.",
        api.api_key_env
    );
    String::new()
}
