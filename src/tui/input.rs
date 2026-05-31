#![allow(dead_code)]

pub struct InputState {
    pub text: String,
    pub cursor: usize,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        if self.cursor <= self.text.len() {
            self.text.insert(self.cursor, c);
            self.cursor += c.len_utf8();
        }
    }

    pub fn delete_before_cursor(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary(self.cursor);
            self.text.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary(self.cursor);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.next_char_boundary(self.cursor);
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn insert_newline(&mut self) {
        if self.cursor <= self.text.len() {
            self.text.insert(self.cursor, '\n');
            self.cursor += 1;
        }
    }

    /// Split text into visual lines, wrapping at `max_width`.
    /// Each logical line (separated by \n) is wrapped independently.
    pub fn visual_lines(&self, max_width: usize) -> Vec<String> {
        if max_width == 0 {
            return vec![self.text.clone()];
        }
        let mut result = Vec::new();
        for logical_line in self.text.split('\n') {
            let line_lines = Self::wrap_line(logical_line, max_width);
            result.extend(line_lines);
        }
        result
    }

    fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
        if max_width == 0 {
            return vec![line.to_string()];
        }
        if line.is_empty() {
            return vec![String::new()];
        }
        let mut lines = Vec::new();
        let mut current = String::new();
        for c in line.chars() {
            let c_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            let cur_width = unicode_width::UnicodeWidthStr::width(current.as_str());
            if cur_width + c_width > max_width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    /// Returns (visual_row, visual_column) of cursor in multi-line display.
    /// max_width is the available character width of the input area.
    pub fn cursor_visual_position(&self, max_width: usize) -> (usize, usize) {
        if max_width == 0 || self.text.is_empty() {
            return (0, self.cursor);
        }
        let text_before = &self.text[..self.cursor];
        let mut row = 0usize;
        let mut line_pos = 0usize;

        for c in text_before.chars() {
            if c == '\n' {
                row += 1;
                line_pos = 0;
                continue;
            }
            let c_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if line_pos + c_width > max_width {
                row += 1;
                line_pos = 0;
            }
            line_pos += c_width;
        }
        (row, line_pos)
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn is_slash_command(&self) -> bool {
        self.text.starts_with('/')
    }

    fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos - 1;
        while p > 0 && !self.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    fn next_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos + 1;
        while p < self.text.len() && !self.text.is_char_boundary(p) {
            p += 1;
        }
        p
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

