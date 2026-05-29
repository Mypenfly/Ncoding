#![allow(dead_code, unused_imports)]

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span, Text},
};

use super::theme::Theme;

pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render_streaming<'a>(&self, text: &'a str, theme: &Theme) -> Text<'a> {
        let lines: Vec<Line<'_>> = text
            .lines()
            .map(|line| {
                if line.starts_with("```") {
                    Line::from(Span::styled(line, Style::default().fg(theme.purple)))
                } else if line.starts_with('#') {
                    Line::from(Span::styled(line, Style::default().fg(theme.orange).add_modifier(Modifier::BOLD)))
                } else if line.starts_with('-') || line.starts_with('*') {
                    Line::from(Span::styled(line, Style::default().fg(theme.fg)))
                } else if line.starts_with('`') && line.ends_with('`') {
                    Line::from(Span::styled(
                        line,
                        Style::default().fg(theme.aqua).add_modifier(Modifier::ITALIC),
                    ))
                } else {
                    Line::from(Span::styled(line, Style::default().fg(theme.fg)))
                }
            })
            .collect();

        Text::from(lines)
    }
}

impl Default for MarkdownRenderer {
    fn default() -> Self {
        Self::new()
    }
}
