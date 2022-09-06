use crate::chunk::Chunk;
use crate::scanner::{Scanner, Token, TokenType};
use crate::value::ValueArray;

struct Parser {
    current: Option<Token>,
    previous: Option<Token>,
    had_error: bool,
    panic_mode: bool,
}

pub(crate) struct Compiler<'c> {
    scanner: Scanner,
    parser: Parser,
    chunk: Box<Chunk<'c>>,
}

impl<'c> Compiler<'c> {
    pub(crate) fn init(scanner: Scanner, mut chunk: Box<Chunk<'c>>) -> Compiler<'c> {
        let parser = Parser {
            current: None,
            previous: None,
            had_error: false,
            panic_mode: false,
        };
        Compiler {
            scanner,
            parser,
            chunk,
        }
    }

    pub(crate) fn compile(&mut self, source: String) -> (bool, Box<Chunk<'c>>) {
        let chars: Vec<char> = source.chars().collect();
        self.scanner.refresh(0, source.len(), chars);
        self.advance();
        (
            self.parser.had_error,
            Box::new(Chunk {
                code: self.chunk.code.clone(),
                constants: ValueArray {
                    values: self.chunk.constants.values.clone(),
                },
                lines: self.chunk.lines.clone(),
            }),
        )
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

    fn emit_byte(&mut self, byte: u8) {
        self.chunk
            .write_chunk(byte, self.parser.previous.unwrap().line);
    }
}
