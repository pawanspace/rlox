use num_derive::FromPrimitive;
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Copy, Clone, FromPrimitive, Hash, Eq)]
pub(crate) enum TokenType {
    // Single-character tokens.
    LeftParen = 1,
    RightParen = 2,
    LeftBrace = 3,
    RightBrace = 4,
    Comma = 5,
    Dot = 6,
    Minus = 7,
    Plus = 8,
    Semicolon = 9,
    Slash = 10,
    Star = 11,
    // One or two character tokens.
    Bang = 12,
    BangEqual = 13,
    Equal = 14,
    EqualEqual = 15,
    Greater = 16,
    GreaterEqual = 17,
    Less = 18,
    LessEqual = 19,
    // Literals.
    Identifier = 20,
    String = 21,
    Number = 22,
    // Keywords.
    And = 23,
    Class = 24,
    Else = 25,
    False = 26,
    For = 27,
    Fun = 28,
    If = 29,
    Nil = 30,
    Or = 31,
    Print = 32,
    Return = 33,
    Super = 34,
    This = 35,
    True = 36,
    Var = 37,
    While = 38,
    Error = 39,
    Eof = 40,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct Token {
    pub token_type: TokenType,
    pub start: usize,
    pub length: usize,
    pub line: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Scanner {
    start: usize,
    current: usize,
    line: u32,
    chars: Vec<char>,
    total_size: usize,
}

fn is_alpha(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

impl Scanner {
    pub(crate) fn init(start: usize, total_size: usize, source: Vec<char>) -> Scanner {
        Scanner {
            start,
            current: start,
            line: 1,
            total_size,
            chars: source,
        }
    }

    pub(crate) fn refresh(&mut self, start: usize, total_size: usize, mut source: Vec<char>) {
        self.chars.clear();
        self.chars.append(&mut source);
        self.total_size = total_size;
        self.current = start;
        self.line = 1;
        self.start = start
    }

    pub(crate) fn scan_token(&mut self) -> Token {
        self.skip_whitespace();
        self.start = self.current;
        if self.is_at_end() {
            return self.make_token(TokenType::Eof);
        }

        // consume current char by moving forward the index
        self.advance();

        // -1 because we want to look at the consumed char
        // look for number token
        if self.chars[self.start].is_digit(10) {
            self.number_token();
            return self.make_token(TokenType::Number);
        }

        // -1 because we want to look at the consumed char
        // look for identifier token that starts with alphabetic
        if is_alpha(self.chars[self.start]) {
            self.identifier();
            let token_type = self.identifier_type();
            return self.make_token(token_type);
        }

        // -1 because we want to look at the consumed char
        match self.chars[self.start] {
            '(' => self.make_token(TokenType::LeftParen),
            ')' => self.make_token(TokenType::RightParen),
            '{' => self.make_token(TokenType::LeftBrace),
            '}' => self.make_token(TokenType::RightBrace),
            ';' => self.make_token(TokenType::Semicolon),
            ',' => self.make_token(TokenType::Comma),
            '.' => self.make_token(TokenType::Dot),
            '-' => self.make_token(TokenType::Minus),
            '+' => self.make_token(TokenType::Plus),
            '/' => self.make_token(TokenType::Slash),
            '*' => self.make_token(TokenType::Star),
            '!' => {
                let token_type = if self.match_char('=') {
                    TokenType::BangEqual
                } else {
                    TokenType::Bang
                };
                self.make_token(token_type)
            }
            '=' => {
                let token_type = if self.match_char('=') {
                    TokenType::EqualEqual
                } else {
                    TokenType::Equal
                };
                self.make_token(token_type)
            }

            '<' => {
                let token_type = if self.match_char('=') {
                    TokenType::LessEqual
                } else {
                    TokenType::Less
                };
                self.make_token(token_type)
            }
            '>' => {
                let token_type = if self.match_char('=') {
                    TokenType::GreaterEqual
                } else {
                    TokenType::Greater
                };
                self.make_token(token_type)
            }
            '"' => {
                // we support multi line string
                while self.peek() != '"' && !self.is_at_end() {
                    if self.peek() == '\n' {
                        self.line += 1;
                    }
                    self.advance();
                }

                // just checking if previous while loop broke due to
                // end of file instead of closing "
                if self.is_at_end() {
                    return self.error_token("Unterminated string.");
                }

                self.advance();
                self.make_token(TokenType::String)
            }
            _ => self.error_token("Unexpected character"),
        }
    }

    fn number_token(&mut self) {
        while self.peek().is_digit(10) {
            self.advance();
        }
        // check for fractional part
        if self.peek() == '.' && self.peek_next().is_digit(10) {
            self.advance();

            while self.peek().is_digit(10) {
                self.advance();
            }
        }
    }

    fn identifier(&mut self) {
        while self.peek().is_digit(10) || is_alpha(self.peek()) {
            self.advance();
        }
    }

    const fn is_at_end(&self) -> bool {
        self.current == self.total_size
    }

    const fn make_token(&self, token_type: TokenType) -> Token {
        Token {
            token_type,
            start: self.start,
            length: (self.current - self.start),
            line: self.line,
        }
    }

    const fn error_token(&self, message: &str) -> Token {
        Token {
            token_type: TokenType::Error,
            start: self.start,
            length: message.len(),
            line: self.line,
        }
    }

    fn advance(&mut self) {
        self.current += 1;
    }

    fn skip_whitespace(&mut self) {
        loop {
            match self.peek() {
                ' ' | '\r' | '\t' => {
                    self.advance();
                }
                '\n' => {
                    self.line += 1;
                    self.advance();
                }
                '/' => {
                    // handle comments
                    if self.peek_next() == '/' {
                        // we have single line comment so once we see
                        // next line or end of file we stop.
                        while self.peek() != '\n' && !self.is_at_end() {
                            self.advance();
                        }
                    }
                }
                _ => {
                    return;
                }
            }
        }
    }

    fn peek(&self) -> char {
        if self.is_at_end() {
            return '\0';
        }
        self.chars[self.current]
    }

    fn peek_next(&self) -> char {
        if self.is_at_end() {
            return '\0';
        }
        self.chars[self.current + 1]
    }

    fn identifier_type(&mut self) -> TokenType {
        match self.chars[self.start] {
            'a' => self.check_keyword(1, 2, "nd", TokenType::And),
            'c' => self.check_keyword(1, 4, "lass", TokenType::Class),
            'e' => self.check_keyword(1, 3, "lse", TokenType::Else),
            'i' => self.check_keyword(1, 1, "f", TokenType::If),
            'n' => self.check_keyword(1, 2, "il", TokenType::Nil),
            'o' => self.check_keyword(1, 1, "r", TokenType::Or),
            'p' => self.check_keyword(1, 4, "rint", TokenType::Print),
            'r' => self.check_keyword(1, 5, "eturn", TokenType::Return),
            's' => self.check_keyword(1, 4, "uper", TokenType::Super),
            'v' => self.check_keyword(1, 2, "ar", TokenType::Var),
            'w' => self.check_keyword(1, 4, "hile", TokenType::While),
            'f' => {
                if self.current - self.start > 1 {
                    // looking for next char
                    return match self.chars[self.start + 1] {
                        'a' => self.check_keyword(2, 3, "lse", TokenType::False),
                        'o' => self.check_keyword(2, 1, "r", TokenType::For),
                        'u' => self.check_keyword(2, 1, "n", TokenType::Fun),
                        _ => TokenType::Identifier,
                    };
                } else {
                    TokenType::Identifier
                }
            }
            't' => {
                if self.current - self.start > 1 {
                    // looking for next char
                    return match self.chars[self.start + 1] {
                        'h' => self.check_keyword(2, 2, "is", TokenType::This),
                        'r' => self.check_keyword(2, 2, "ue", TokenType::True),
                        _ => TokenType::Identifier,
                    };
                } else {
                    TokenType::Identifier
                }
            }
            _ => TokenType::Identifier,
        }
    }

    fn check_keyword(
        &self,
        start: usize,
        length: usize,
        rest: &str,
        token_type: TokenType,
    ) -> TokenType {
        let start_index = self.start + start;
        let end_index_exclusive = start_index + length;

        if self.chars.len() >= end_index_exclusive {
            let slice = &self.chars[start_index..end_index_exclusive];
            let rest_slice: Vec<char> = rest.chars().collect();
            let o = slice.cmp(&rest_slice);
            if o == Ordering::Equal {
                return token_type;
            }
        }

        TokenType::Identifier
    }

    fn match_char(&mut self, c: char) -> bool {
        if self.is_at_end() {
            return false;
        }
        if self.chars[self.current] == c {
            self.advance();
            return true;
        }

        false
    }
}
