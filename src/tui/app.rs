use std::io;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
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
use unicode_width::UnicodeWidthStr;
use tokio::sync::mpsc;

use crate::{
    api::client::{DeepSeekClient, TuiEvent},
    command::{execute_commands, format_command_results, CommandWatcher},
    config::loader::AppConfig,
    prompt::builder::PromptBuilder,
    session::manager::{Message, Role, SessionManager},
};

use super::theme::Theme;

pub type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub struct App {
    config: AppConfig,
    api_client: DeepSeekClient,
    #[allow(dead_code)]
    prompt_builder: PromptBuilder,
    cmd_watcher: CommandWatcher,
    session_mgr: SessionManager,
    max_ctx: usize,
    auto_continue_count: u32,
    input: String,
    cursor_pos: usize,
    scroll_offset: usize,
    auto_scroll: bool,
    is_streaming: bool,
    thinking_text: String,
    content_text: String,
    token_info: String,
    status_text: String,
    tui_tx: mpsc::UnboundedSender<TuiEvent>,
    tui_rx: mpsc::UnboundedReceiver<TuiEvent>,
}

pub fn init_terminal() -> io::Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

pub fn restore_terminal(terminal: &mut TuiTerminal) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
        let _ = session_mgr.new_session(&session_name);
        let system_prompt = prompt_builder.build();
        session_mgr.add_message(Message {
            role: Role::System,
            content: Some(system_prompt),
            reasoning_content: None,
        }).ok();

        let max_ctx = config.session.max_context_messages;
        let status_text = format!(
            "{} · {} · {} · thinking",
            workspace,
            config.api.model,
            session_mgr.current_name().unwrap_or("new"),
        );

        Self {
            api_client,
            prompt_builder,
            cmd_watcher: CommandWatcher::new(),
            session_mgr,
            max_ctx,
            auto_continue_count: 0,
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            auto_scroll: true,
            is_streaming: false,
            thinking_text: String::new(),
            content_text: String::new(),
            token_info: String::new(),
            status_text,
            tui_tx,
            tui_rx,
            config,
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
                            if !self.input.is_empty() => {
                                let input_text = std::mem::take(&mut self.input);
                                self.cursor_pos = 0;
                                if input_text.starts_with('/') {
                                    self.handle_slash_command(input_text).await;
                                } else {
                                    self.handle_user_input(input_text).await;
                                }
                            }
                        KeyCode::Char(c)
                            if self.cursor_pos <= self.input.len() => {
                                self.input.insert(self.cursor_pos, c);
                                self.cursor_pos += c.len_utf8();
                            }
                        KeyCode::Backspace
                            if self.cursor_pos > 0 => {
                                self.cursor_pos = self.prev_char_boundary(self.cursor_pos);
                                self.input.remove(self.cursor_pos);
                            }
                        KeyCode::Left if self.cursor_pos > 0 => {
                            self.cursor_pos = self.prev_char_boundary(self.cursor_pos);
                        }
                        KeyCode::Right if self.cursor_pos < self.input.len() => {
                            self.cursor_pos = self.next_char_boundary(self.cursor_pos);
                        }
                        KeyCode::Home => self.cursor_pos = 0,
                        KeyCode::End => self.cursor_pos = self.input.len(),
                        KeyCode::Up if self.scroll_offset > 0 => {
                            self.scroll_offset -= 1;
                            self.auto_scroll = false;
                        }
                        KeyCode::Down => {
                            self.scroll_offset += 1;
                        }
                        KeyCode::PageUp => {
                            self.scroll_offset = self.scroll_offset.saturating_sub(10);
                            self.auto_scroll = false;
                        }
                        KeyCode::PageDown => self.scroll_offset += 10,
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
        self.session_mgr.truncate_context(self.max_ctx);
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
        self.status_text = format!(
            "{} · {} · {} · thinking",
            workspace,
            self.config.api.model,
            self.session_mgr.current_name().unwrap_or("new"),
        );
    }

    async fn handle_user_input(&mut self, input_text: String) {
        self.push_msg(Message {
            role: Role::User,
            content: Some(input_text.clone()),
            reasoning_content: None,
        });

        if self.auto_continue_count == 0 {
            let current = self.session_mgr.current_name().unwrap_or("");
            if current.starts_with("session-") && current.len() > 20 {
                if let Ok(name) = self
                    .api_client
                    .generate_session_name(&input_text, &self.config.api.sub_model)
                    .await
                {
                    if let Ok(()) = self.session_mgr.rename_session(&name) {
                        self.update_status();
                    }
                }
            }
            self.auto_continue_count = 0;
        }

        self.auto_continue_count = 0;
        self.auto_scroll = true;
        self.continue_conversation().await;
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
                self.push_msg(Message {
                    role: Role::Assistant,
                    content: Some(format!("[Error] {}", err)),
                    reasoning_content: None,
                });
            }
        }
    }

    async fn handle_stream_done(&mut self) {
        self.is_streaming = false;

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

        let commands = self.cmd_watcher.finalize();
        if !commands.is_empty() {
            let results = execute_commands(commands).await;
            let injection = format_command_results(&results);

            if !injection.is_empty() {
                self.push_msg(Message {
                    role: Role::User,
                    content: Some(injection),
                    reasoning_content: None,
                });

                if self.auto_continue_count < 5 {
                    self.auto_continue_count += 1;
                    self.continue_conversation().await;
                }
            }
        }
    }

    async fn continue_conversation(&mut self) {
        self.thinking_text.clear();
        self.content_text.clear();
        self.is_streaming = true;
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
                /session switch <name> - switch session\n\
                /session rename <name> - rename current\n\
                /session delete <name> - delete session\n\
                /session current - show current\n\
                /undo - undo last turn";
                self.push_msg(Message {
                    role: Role::Assistant,
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
                            role: Role::Assistant,
                            content: Some("[undo] removed last turn".into()),
                            reasoning_content: None,
                        });
                    }
                    None => {
                        self.push_msg(Message {
                            role: Role::Assistant,
                            content: Some("[undo] nothing to undo".into()),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "/model" | "/skills" => {
                self.push_msg(Message {
                    role: Role::Assistant,
                    content: Some(format!("{}: coming in Phase 3", cmd)),
                    reasoning_content: None,
                });
            }
            _ => {
                self.push_msg(Message {
                    role: Role::Assistant,
                    content: Some(format!("unknown command: {}. Type /help for available commands.",
                        cmd)),
                    reasoning_content: None,
                });
            }
        }
    }

    async fn handle_session_cmd(&mut self, sub: &str, parts: &[&str]) {
        match sub {
            "list" => {
                match self.session_mgr.list_sessions() {
                    Ok(sessions) => {
                        if sessions.is_empty() {
                            self.push_msg(Message {
                                role: Role::Assistant,
                                content: Some("No sessions found.".into()),
                                reasoning_content: None,
                            });
                        } else {
                            let mut out = String::from("Sessions:\n");
                            for (name, updated) in &sessions {
                                let current_mark = if Some(name.as_str()) == self.session_mgr.current_name() {
                                    " *"
                                } else {
                                    ""
                                };
                                out.push_str(&format!("  {} ({}){}\n", name, updated.format("%Y-%m-%d %H:%M"), current_mark));
                            }
                            self.push_msg(Message {
                                role: Role::Assistant,
                                content: Some(out),
                                reasoning_content: None,
                            });
                        }
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Assistant,
                            content: Some(format!("Error listing sessions: {}", e)),
                            reasoning_content: None,
                        });
                    }
                }
            }
            "switch" => {
                let name = parts.get(2).copied().unwrap_or("");
                if name.is_empty() {
                    self.push_msg(Message {
                        role: Role::Assistant,
                        content: Some("Usage: /session switch <name>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                match self.session_mgr.load_session(name) {
                    Ok(()) => {
                        self.update_status();
                        let msgs = self.msgs();
                        self.push_msg(Message {
                            role: Role::Assistant,
                            content: Some(format!("Switched to session '{}' ({} messages)", name, msgs.len())),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Assistant,
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
                        role: Role::Assistant,
                        content: Some("Usage: /session rename <new_name>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                match self.session_mgr.rename_session(new_name) {
                    Ok(()) => {
                        self.update_status();
                        self.push_msg(Message {
                            role: Role::Assistant,
                            content: Some(format!("Renamed to '{}'", new_name)),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Assistant,
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
                        role: Role::Assistant,
                        content: Some("Usage: /session delete <name>".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                if Some(name) == self.session_mgr.current_name() {
                    self.push_msg(Message {
                        role: Role::Assistant,
                        content: Some("Cannot delete current session. Switch first.".into()),
                        reasoning_content: None,
                    });
                    return;
                }
                match self.session_mgr.delete_session(name) {
                    Ok(()) => {
                        self.push_msg(Message {
                            role: Role::Assistant,
                            content: Some(format!("Deleted session '{}'", name)),
                            reasoning_content: None,
                        });
                    }
                    Err(e) => {
                        self.push_msg(Message {
                            role: Role::Assistant,
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
                    role: Role::Assistant,
                    content: Some(format!("Current session: '{}' ({} messages)", name, msgs.len())),
                    reasoning_content: None,
                });
            }
            _ => {
                self.push_msg(Message {
                    role: Role::Assistant,
                    content: Some("Usage: /session [list|switch|rename|delete|current]".into()),
                    reasoning_content: None,
                });
            }
        }
    }

    fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.input.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    fn next_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos + 1;
        while p < self.input.len() && !self.input.is_char_boundary(p) {
            p += 1;
        }
        p
    }

    fn draw(&self, f: &mut Frame) {
        let theme = Theme::everforest();
        let area = f.area();

        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(3),
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

        let text_area = chunks[1];
        let mut lines: Vec<Line> = Vec::new();

        let display_msgs = self.msgs();
        for msg in &display_msgs {
            match msg.role {
                Role::System => {
                    lines.push(Line::from(Span::styled(
                        "[system prompt]",
                        Style::default().fg(theme.grey2),
                    )));
                }
                Role::User => {
                    let text = msg.content.as_deref().unwrap_or("");
                    if text.starts_with("<(<(SYSTEM") {
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
                                lines.push(Line::from(Span::styled(
                                    if rline.trim().is_empty() {
                                        String::new()
                                    } else {
                                        format!("<< {} >>", rline)
                                    },
                                    Style::default().fg(theme.grey2),
                                )));
                            }
                        }
                    }
                    let text = msg.content.as_deref().unwrap_or("");
                    for line_str in text.lines() {
                        lines.push(self.render_content_line(line_str, &theme));
                    }
                }
            }
        }

        if self.is_streaming {
            if !self.thinking_text.is_empty() {
                for rline in self.thinking_text.lines() {
                    if !rline.trim().is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("<< {} >>", rline),
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

        let effective_offset = if self.auto_scroll {
            let visible_h = text_area.height.saturating_sub(1) as usize;
            lines.len().saturating_sub(visible_h)
        } else {
            self.scroll_offset
        };

        let visible_lines: Vec<Line> = lines
            .into_iter()
            .skip(effective_offset)
            .collect();

        let text_widget = Paragraph::new(Text::from(visible_lines))
            .block(Block::default())
            .wrap(Wrap { trim: false });
        f.render_widget(text_widget, text_area);

        let token_bar = chunks[2];
        let token_text = if self.token_info.is_empty() {
            "Tokens · --"
        } else {
            &self.token_info
        };
        let token_line = Line::from(Span::styled(
            token_text,
            Style::default().fg(theme.orange),
        ));
        f.render_widget(
            Paragraph::new(token_line).block(Block::default()),
            token_bar,
        );

        let input_area = chunks[3];
        let input_display = if self.input.is_empty() {
            "> _"
        } else {
            &self.input
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
            .style(input_style);
        f.render_widget(input_widget, input_area);

        if !self.is_streaming {
            let text_before_cursor = &self.input[..self.cursor_pos];
            let display_width = UnicodeWidthStr::width(text_before_cursor);
            let cursor_x = input_area.x + 1 + display_width as u16;
            let cursor_y = input_area.y + 1;
            let clamped_x = cursor_x.min(input_area.right().saturating_sub(2));
            f.set_cursor_position((clamped_x, cursor_y));
        }

        let status_bar = chunks[4];
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
                Style::default().fg(theme.orange).add_modifier(Modifier::BOLD),
            ))
        } else if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
            Line::from(Span::styled(
                line,
                Style::default().fg(theme.aqua).add_modifier(Modifier::ITALIC),
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

    tracing::warn!("No API key found. Set api_key in config or set {} env var.", api.api_key_env);
    String::new()
}
