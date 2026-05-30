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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_input_is_empty() {
        let input = InputState::new();
        assert!(input.text.is_empty());
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_insert_char_at_end() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        assert_eq!(input.text, "ab");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_insert_char_in_middle() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('c');
        input.move_left();
        input.insert_char('b');
        assert_eq!(input.text, "abc");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_delete_before_cursor() {
        let mut input = InputState::new();
        input.insert_char('x');
        input.insert_char('y');
        input.delete_before_cursor();
        assert_eq!(input.text, "x");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_delete_before_cursor_at_start() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.move_home();
        input.delete_before_cursor();
        assert_eq!(input.text, "a");
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_move_left_right() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        input.move_left();
        assert_eq!(input.cursor, 2);
        input.move_left();
        assert_eq!(input.cursor, 1);
        input.move_right();
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_move_left_at_start_does_nothing() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.move_home();
        input.move_left();
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_move_right_at_end_does_nothing() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.move_right();
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_move_home_and_end() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_char('c');
        assert_eq!(input.cursor, 3);
        input.move_home();
        assert_eq!(input.cursor, 0);
        input.move_end();
        assert_eq!(input.cursor, 3);
    }

    #[test]
    fn test_clear() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('i');
        input.clear();
        assert!(input.text.is_empty());
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_take() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('i');
        let taken = input.take();
        assert_eq!(taken, "hi");
        assert!(input.text.is_empty());
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_is_slash_command() {
        let mut input = InputState::new();
        assert!(!input.is_slash_command());
        input.insert_char('/');
        input.insert_char('h');
        assert!(input.is_slash_command());
    }

    #[test]
    fn test_is_not_slash_command() {
        let mut input = InputState::new();
        input.insert_char('h');
        input.insert_char('e');
        input.insert_char('l');
        input.insert_char('l');
        input.insert_char('o');
        assert!(!input.is_slash_command());
    }

    #[test]
    fn test_insert_chinese_characters_no_panic() {
        let mut input = InputState::new();
        input.insert_char('你');
        input.insert_char('好');
        assert_eq!(input.text, "你好");
        assert_eq!(input.cursor, "你好".len());
    }

    #[test]
    fn test_insert_chinese_and_ascii() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('你');
        input.insert_char('b');
        assert_eq!(input.text, "a你b");
        assert_eq!(input.cursor, "a你b".len());
    }

    #[test]
    fn test_delete_chinese_char() {
        let mut input = InputState::new();
        input.insert_char('你');
        input.insert_char('好');
        input.delete_before_cursor();
        assert_eq!(input.text, "你");
        assert_eq!(input.cursor, "你".len());
    }

    #[test]
    fn test_move_left_over_chinese() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('你');
        input.insert_char('b');
        input.move_left();
        assert_eq!(input.cursor, "a你".len());
        input.move_left();
        assert_eq!(input.cursor, "a".len());
        input.move_left();
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_move_right_over_chinese() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('你');
        input.insert_char('b');
        input.move_home();
        input.move_right();
        assert_eq!(input.cursor, 1);
        input.move_right();
        assert_eq!(input.cursor, "a你".len());
    }

    #[test]
    fn test_insert_chinese_in_middle() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('c');
        input.move_left();
        input.insert_char('你');
        assert_eq!(input.text, "a你c");
    }

    #[test]
    fn test_many_chinese_characters() {
        let mut input = InputState::new();
        for c in "你好世界！测试一下".chars() {
            input.insert_char(c);
        }
        assert_eq!(input.text, "你好世界！测试一下");
        assert_eq!(input.cursor, "你好世界！测试一下".len());
    }

    // --- Multi-line tests ---

    #[test]
    fn test_insert_newline_middle() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.insert_char('b');
        input.insert_newline();
        input.insert_char('c');
        assert_eq!(input.text, "ab\nc");
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn test_insert_newline_at_start() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.move_home();
        input.insert_newline();
        assert_eq!(input.text, "\na");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn test_visual_lines_single() {
        let mut input = InputState::new();
        for c in "hello".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "hello");
    }

    #[test]
    fn test_visual_lines_wrap() {
        let mut input = InputState::new();
        for c in "hello world".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(5);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "hello");
        assert_eq!(lines[1], " worl");
        assert_eq!(lines[2], "d");
    }

    #[test]
    fn test_visual_lines_with_newlines() {
        let mut input = InputState::new();
        for c in "ab\ncd".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(10);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "cd");
    }

    #[test]
    fn test_visual_lines_trailing_newline() {
        let mut input = InputState::new();
        for c in "ab\n".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(10);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "");
    }

    #[test]
    fn test_visual_lines_newline_wrap() {
        let mut input = InputState::new();
        for c in "hello\nworld".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(3);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "hel");
        assert_eq!(lines[1], "lo");
        assert_eq!(lines[2], "wor");
        assert_eq!(lines[3], "ld");
    }

    #[test]
    fn test_cursor_visual_position_single_line() {
        let mut input = InputState::new();
        for c in "hello".chars() {
            input.insert_char(c);
        }
        let (row, col) = input.cursor_visual_position(10);
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn test_cursor_visual_position_multiline() {
        let mut input = InputState::new();
        for c in "ab\ncd".chars() {
            input.insert_char(c);
        }
        let (row, col) = input.cursor_visual_position(10);
        assert_eq!(row, 1);
        assert_eq!(col, 2);
    }

    #[test]
    fn test_cursor_visual_wrapped() {
        let mut input = InputState::new();
        for c in "hello world".chars() {
            input.insert_char(c);
        }
        let (row, col) = input.cursor_visual_position(5);
        assert_eq!(row, 2);
 assert_eq!(col, 1);
    }

    #[test]
    fn test_cursor_visual_position_at_wrap_boundary() {
        let mut input = InputState::new();
        for c in "hello".chars() {
            input.insert_char(c);
        }
        let (row, col) = input.cursor_visual_position(5);
        assert_eq!(row, 0);
        assert_eq!(col, 5);

        input.insert_char('x');
        let (row, col) = input.cursor_visual_position(5);
        assert_eq!(row, 1);
        assert_eq!(col, 1);
    }

    #[test]
    fn test_visual_lines_width_one() {
        let mut input = InputState::new();
        for c in "abc".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(1);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "a");
        assert_eq!(lines[1], "b");
        assert_eq!(lines[2], "c");
    }

    #[test]
    fn test_visual_lines_cjk_wrap() {
        let mut input = InputState::new();
        for c in "你好世界".chars() {
            input.insert_char(c);
        }
        let lines = input.visual_lines(5);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "你好");
        assert_eq!(lines[1], "世界");
    }

    #[test]
    fn test_cursor_visual_position_multiline_with_wrap() {
        let mut input = InputState::new();
        for c in "ab\ncde".chars() {
            input.insert_char(c);
        }
        let (row, col) = input.cursor_visual_position(3);
        assert_eq!(row, 1);
        assert_eq!(col, 3);
    }

    #[test]
    fn test_is_empty_after_clear() {
        let mut input = InputState::new();
        input.insert_char('a');
        input.clear();
        assert!(input.is_empty());
        assert_eq!(input.cursor, 0);
    }

    #[test]
    fn test_visual_lines_empty_text() {
        let input = InputState::new();
        let lines = input.visual_lines(10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "");
    }

    #[test]
    fn test_cursor_visual_position_empty_text() {
        let input = InputState::new();
        let (row, col) = input.cursor_visual_position(10);
        assert_eq!(row, 0);
        assert_eq!(col, 0);
    }
}

