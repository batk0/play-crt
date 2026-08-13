use crate::constants::{COLS, ROWS};

pub struct Grid {
    pub cells: [[char; COLS]; ROWS],
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub dirty: bool,
}

impl Grid {
    pub fn new() -> Self {
        Self {
            cells: [[' '; COLS]; ROWS],
            cursor_x: 0,
            cursor_y: 0,
            dirty: true,
        }
    }

    pub fn clear(&mut self) {
        for row in &mut self.cells {
            *row = [' '; COLS];
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.dirty = true;
    }

    fn scroll_up(&mut self) {
        for y in 1..ROWS {
            self.cells[y - 1] = self.cells[y];
        }
        self.cells[ROWS - 1] = [' '; COLS];
        self.dirty = true;
    }

    pub fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= ROWS {
            self.scroll_up();
            self.cursor_y = ROWS - 1;
        }
        self.dirty = true;
    }

    pub fn put_char(&mut self, ch: char) {
        match ch {
            '\r' => {
                self.cursor_x = 0;
                self.dirty = true;
            }
            '\n' => self.newline(),
            '\x0C' => self.clear(),
            '\x08' | '\x7F' => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    self.cells[self.cursor_y][self.cursor_x] = ' ';
                } else if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                    self.cursor_x = COLS - 1;
                    self.cells[self.cursor_y][self.cursor_x] = ' ';
                }
                self.dirty = true;
            }
            c if c.is_control() => {}
            c => {
                if self.cursor_x >= COLS {
                    self.newline();
                }
                if self.cursor_y < ROWS && self.cursor_x < COLS {
                    self.cells[self.cursor_y][self.cursor_x] = c;
                    self.cursor_x += 1;
                    if self.cursor_x >= COLS {
                        self.cursor_x = 0;
                        self.cursor_y += 1;
                        if self.cursor_y >= ROWS {
                            self.scroll_up();
                            self.cursor_y = ROWS - 1;
                        }
                    }
                    self.dirty = true;
                }
            }
        }
    }

    pub fn put_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.put_char(ch);
        }
    }

    #[allow(dead_code)]
    pub fn line_string(&self, y: usize) -> String {
        self.cells[y].iter().collect()
    }

    pub fn line_trimmed(&self, y: usize) -> String {
        let s: String = self.cells[y].iter().collect();
        s.trim_end().to_string()
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}
