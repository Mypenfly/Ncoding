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
}
