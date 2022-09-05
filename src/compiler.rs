use crate::chunk::Chunk;
use crate::scanner::{Scanner, Token, TokenType};

struct Parser {
    current: Option<Token>,
    previous: Option<Token>,
    had_error: bool,
    panic_mode: bool,
}

pub(crate) struct Compiler<'c> {
    scanner: &'c mut Scanner<'c>,
    parser: Parser,
}

impl<'c> Compiler<'c> {
    pub(crate) fn init(scanner: &'c mut Scanner<'c>) -> Compiler<'c> {
        let parser = Parser {
            current: None,
            previous: None,
            had_error: false,
            panic_mode: false,
        };
        Compiler { scanner, parser }
    }

    pub(crate) fn compile(&mut self, source: String, chunk: &Chunk) -> bool {
        let mut chars: Vec<char> = source.chars().collect();
        self.scanner.refresh(0, source.len(), &mut chars);
        self.advance();
        !self.parser.had_error
    }

    fn advance(&mut self) {
        self.parser.previous = self.parser.current;
        loop {
            self.parser.current = Some(self.scanner.scan_token());

            if self.parser.current.unwrap().token_type != TokenType::Error {
                break;
            }
        }

        self.error_at_current("@todo some error here")
    }

    fn error_at_current(&mut self, message: &str) {
        self.error_at(message, self.parser.current.unwrap())
    }

    fn error(&mut self, message: &str) {
        self.error_at(message, self.parser.previous.unwrap())
    }

    fn error_at(&mut self, message: &str, token: Token) {
        if self.parser.panic_mode {
            return;
        }
        self.parser.panic_mode = true;
        eprint!("[line: {}] Error", token.line);

        match token.token_type {
            TokenType::Eof => eprint!(" at end"),
            TokenType::Error => eprint!(""),
            _ => eprint!(" at {}.{}", token.length, token.start),
        }

        eprintln!(": {}", message);
        self.parser.had_error = true;
    }

    fn consume(&mut self, token_type: TokenType, message: &str) {
        if self.parser.current.unwrap().token_type == token_type {
            self.advance();
            return;
        }

        self.error_at_current(message);
    }
}
