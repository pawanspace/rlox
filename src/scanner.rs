//! # The Scanner (a.k.a. lexer / tokenizer)
//!
//! First stage of the pipeline: it turns raw source *characters* into
//! *tokens* — the small meaningful units of the language like `(`, `+`, a
//! number, an identifier, or a keyword such as `while`.
//!
//! ```text
//!   "var x = 10;"  ->  [Var] [Identifier "x"] [Equal] [Number "10"] [Semicolon] [Eof]
//! ```
//!
//! ## Why "on demand" (scan one token at a time)
//! A classic lexer tokenizes the whole file up front into a list. This scanner
//! instead produces tokens *lazily*: the compiler calls `scan_token()` each
//! time it wants the next one. This is clox's design, and it matters because
//! this interpreter is single-pass — the compiler consumes a token, emits
//! bytecode, and asks for the next, so there's no reason to build (and store)
//! the entire token list ahead of time. It also keeps the scanner and compiler
//! in lockstep and saves memory.
//!
//! ## Tokens are (type, start, length, line) — not owned strings
//! A `Token` does NOT copy the text it represents. It stores the token's
//! *offsets* into the original source (`start` + `length`) plus its `line`.
//! To get the actual text (a variable's name, a number's digits) the compiler
//! slices the source using those offsets. This avoids allocating a `String`
//! for every token — the source is kept around and everything borrows from it.

use num_derive::FromPrimitive;
use std::cmp::Ordering;

/// Every kind of token the language recognizes.
///
/// `#[repr(u8)]` with explicit discriminants pins each variant to a specific
/// byte value, so a `TokenType` is just a `u8` at runtime — cheap to copy and
/// compare. `FromPrimitive` allows turning a `u8` back into a `TokenType`.
/// `Copy` means tokens are copied (not moved) on assignment, which is why you
/// see `Token` passed around by value freely elsewhere.
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

/// A single lexical token.
///
/// It carries no owned text — see the module docs. `start`/`length` are offsets
/// into the source so the actual lexeme can be sliced out later.
#[derive(Debug, Copy, Clone)]
pub(crate) struct Token {
    /// What kind of token this is.
    pub token_type: TokenType,
    /// Index of the token's first character in the source.
    pub start: usize,
    /// Number of characters the token spans (so the lexeme is `source[start..start+length]`).
    pub length: usize,
    /// 1-based source line the token starts on, for error messages.
    pub line: u32,
}

/// The scanner's cursor state as it walks the source.
#[derive(Debug, Clone)]
pub(crate) struct Scanner {
    /// Index where the token currently being scanned begins.
    start: usize,
    /// Index of the next character to look at (the read cursor).
    current: usize,
    /// Current line number, bumped on every `\n` for error reporting.
    line: u32,
    /// The source, pre-split into `char`s so indexing is by character, not byte.
    chars: Vec<char>,
    /// Total number of characters; `current == total_size` means end-of-input.
    total_size: usize,
}

/// True if `c` may start or appear in an identifier.
///
/// Lox identifiers are letters, digits (after the first char), or `_`. This
/// helper covers the letter/underscore case; digits are handled separately.
fn is_alpha(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

impl Scanner {
    /// Create a scanner over `source` (already split into `char`s).
    ///
    /// Lines are 1-based, so `line` starts at 1.
    pub(crate) fn init(start: usize, total_size: usize, source: Vec<char>) -> Scanner {
        Scanner {
            start,
            current: start,
            line: 1,
            total_size,
            chars: source,
        }
    }

    /// Reset the scanner to run over a new source, reusing the same struct.
    ///
    /// Used because the VM constructs a scanner once and then hands it fresh
    /// source (see `Compiler::compile`), rather than allocating a new scanner.
    pub(crate) fn refresh(&mut self, start: usize, total_size: usize, mut source: Vec<char>) {
        self.chars.clear();
        self.chars.append(&mut source);
        self.total_size = total_size;
        self.current = start;
        self.line = 1;
        self.start = start
    }

    /// Produce the next token from the source.
    ///
    /// This is the scanner's public interface — the compiler calls it once per
    /// token. The flow: skip leading whitespace/comments, mark `start`, then
    /// dispatch on the first character:
    ///   - end of input        -> `Eof`
    ///   - a digit             -> scan a whole number
    ///   - a letter/`_`        -> scan an identifier, then check if it's a keyword
    ///   - anything else       -> a punctuation/operator token (possibly two chars)
    pub(crate) fn scan_token(&mut self) -> Token {
        self.skip_whitespace();
        // Everything from here to the return is one token; record where it starts.
        self.start = self.current;
        if self.is_at_end() {
            return self.make_token(TokenType::Eof);
        }

        // Consume the first character by advancing the cursor. After this,
        // `self.start` still points at that first char (so comments below say
        // "look at the consumed char" — i.e. index `self.start`).
        self.advance();

        // A token beginning with a digit is a number literal.
        if self.chars[self.start].is_digit(10) {
            self.number_token();
            return self.make_token(TokenType::Number);
        }

        // A token beginning with a letter/underscore is an identifier — which
        // may turn out to be a reserved keyword (see `identifier_type`).
        if is_alpha(self.chars[self.start]) {
            self.identifier();
            let token_type = self.identifier_type();
            return self.make_token(token_type);
        }

        // Otherwise it's punctuation or an operator; dispatch on the character.
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
            // Two-character operators: after seeing `!`, peek for a following
            // `=` via `match_char`. If present, it's `!=` (BangEqual) and the
            // `=` is consumed; otherwise it's a lone `!` (Bang). Same pattern
            // for `=`/`==`, `<`/`<=`, `>`/`>=` below.
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

    /// Consume the rest of a number literal (integer or decimal).
    ///
    /// This is "maximal munch": keep consuming digits as long as they appear,
    /// so the longest valid number is taken. A fractional part is only consumed
    /// if there is a `.` *followed by another digit* — that's why `peek_next`
    /// is checked. This prevents grabbing the `.` in `123.method()` (method
    /// access) as part of the number.
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

    /// Consume the rest of an identifier (letters, digits, `_`).
    ///
    /// Also maximal munch: the identifier extends as far as valid characters
    /// go. Whether the result is a keyword is decided afterward.
    fn identifier(&mut self) {
        while self.peek().is_digit(10) || is_alpha(self.peek()) {
            self.advance();
        }
    }

    /// Have we consumed the entire source?
    ///
    /// `const fn` means this can also be evaluated at compile time; here it's
    /// just a cheap read.
    const fn is_at_end(&self) -> bool {
        self.current == self.total_size
    }

    /// Build a token of `token_type` spanning `start..current`.
    ///
    /// The length is derived from how far the cursor advanced while scanning,
    /// which is exactly why the scanner tracks offsets instead of copying text.
    const fn make_token(&self, token_type: TokenType) -> Token {
        Token {
            token_type,
            start: self.start,
            length: (self.current - self.start),
            line: self.line,
        }
    }

    /// Build an error token carrying a diagnostic `message`.
    // BUG/smell: this stuffs `message.len()` into the `length` field, which is
    // meant to be the lexeme length (an offset span into the source), not the
    // length of the error string. Downstream code that slices the source using
    // `start..start+length` on an error token would read the wrong range. The
    // message itself is also dropped — it's never stored, only its length.
    const fn error_token(&self, message: &str) -> Token {
        Token {
            token_type: TokenType::Error,
            start: self.start,
            length: message.len(),
            line: self.line,
        }
    }

    /// Move the read cursor forward one character.
    fn advance(&mut self) {
        self.current += 1;
    }

    /// Advance past spaces, tabs, carriage returns, newlines, and `//` comments
    /// so the next `scan_token` starts on meaningful input.
    ///
    /// Whitespace is not a token in Lox, so it's discarded here rather than
    /// producing tokens the compiler would have to skip. Newlines bump `line`
    /// so error messages report the right line. `//` comments run to end of
    /// line. Note there is no `/* */` block-comment handling.
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

    /// Look at the current character without consuming it.
    ///
    /// Returns `'\0'` at end of input as a sentinel, so callers can compare
    /// against characters without a separate bounds check.
    fn peek(&self) -> char {
        if self.is_at_end() {
            return '\0';
        }
        self.chars[self.current]
    }

    /// Look one character past the current one without consuming.
    ///
    /// Used for two-character decisions like "is this `.` followed by a digit?"
    fn peek_next(&self) -> char {
        if self.is_at_end() {
            return '\0';
        }
        self.chars[self.current + 1]
    }

    /// Decide whether a just-scanned identifier is actually a reserved keyword.
    ///
    /// ## Why a hand-rolled trie instead of a hash lookup
    /// Rather than hashing the identifier and looking it up in a keyword map,
    /// this switches on the *first* character and then compares only the
    /// remaining letters (`check_keyword`). This is a tiny trie (prefix tree):
    /// most identifiers fail at the very first character and are classified as
    /// `Identifier` after a single comparison — no hashing, no allocation. It's
    /// the classic clox technique, and it's fast because keyword sets are small
    /// and fixed at compile time.
    ///
    /// The nested matches for `'f'` and `'t'` exist because several keywords
    /// share a first letter (`false`/`for`/`fun`, `this`/`true`), so we branch
    /// again on the second character.
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

    /// Confirm that the identifier's tail matches a keyword's remaining letters.
    ///
    /// Given the identifier starts at `self.start`, this compares the substring
    /// beginning `start` characters in (skipping the already-matched prefix)
    /// and running `length` characters against `rest`. If they're equal, it's
    /// the keyword `token_type`; otherwise it's a plain `Identifier`.
    ///
    /// Example: for `while`, after matching `w` we call
    /// `check_keyword(1, 4, "hile", While)` — compare the 4 chars after index 1
    /// to "hile". The `self.chars.len() >= end_index_exclusive` guard prevents
    /// indexing past the source for a shorter identifier like `w`.
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

    /// Conditionally consume the current character if it equals `c`.
    ///
    /// Returns `true` and advances the cursor on a match, else returns `false`
    /// and leaves the cursor put. This is the primitive behind two-character
    /// operators (`!=`, `==`, `<=`, `>=`) in `scan_token`.
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
