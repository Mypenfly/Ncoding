#![allow(dead_code, unused_imports)]

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::config::loader::AppConfig;

pub struct ModelPanel {
    pub visible: bool,
    pub selected_model: usize,
    pub thinking_enabled: bool,
    pub thinking_effort: usize, // 0: high, 1: max
    pub models: Vec<String>,
}

impl ModelPanel {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            visible: false,
            selected_model: 0,
            thinking_enabled: config.thinking.enabled,
            thinking_effort: if config.thinking.reasoning_effort == "max" { 1 } else { 0 },
            models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into()],
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn next_model(&mut self) {
        if !self.models.is_empty() {
            self.selected_model = (self.selected_model + 1) % self.models.len();
        }
    }

    pub fn prev_model(&mut self) {
        if !self.models.is_empty() {
            self.selected_model = if self.selected_model == 0 {
                self.models.len() - 1
            } else {
                self.selected_model - 1
            };
        }
    }

    pub fn selected_model_id(&self) -> Option<&str> {
        self.models.get(self.selected_model).map(|s| s.as_str())
    }
}
