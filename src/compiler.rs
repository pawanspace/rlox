use crate::chunk::Chunk;
use crate::common::{OpCode, Value};
use crate::scanner::{Scanner, Token, TokenType};
use crate::value::ValueArray;

enum Precedence {
    None,
    Assignment,
    Or,
    And,
    Equality,
    Comparison,
    Term,
    Factor,
    Unary,
    Call,
    Primary,
}

struct Parser {
    current: Option<Token>,
    previous: Option<Token>,
    had_error: bool,
    panic_mode: bool,
}

pub(crate) struct Compiler {
    scanner: Scanner,
    parser: Parser,
    chunk: Box<Chunk>,
    source: String,
}

impl Compiler {
    pub(crate) fn init(scanner: Scanner, chunk: Box<Chunk>) -> Compiler {
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
            source: "".to_string(),
        }
    }

    pub(crate) fn compile(&mut self, source: String) -> (bool, Box<Chunk>) {
        self.source = source;
        let chars: Vec<char> = self.source.chars().collect();
        self.scanner.refresh(0, self.source.len(), chars);
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

    fn emit_bytes(&mut self, byte_1: u8, byte_2: u8) {
        self.emit_byte(byte_1);
        self.emit_byte(byte_2);
    }

    fn end_compiler(&mut self) {
        self.emit_return()
    }

    fn emit_return(&mut self) {
        self.emit_byte(OpCode::Return as u8)
    }

    fn expression(&mut self) {}

    fn emit_constant(&mut self, value: Value) {
        self.chunk
            .write_constant(value, self.parser.previous.unwrap().line)
    }

    fn str_to_float(&mut self, token: Token) -> f64 {
        let value = &self.source[token.start..token.start + token.length];
        value.parse::<f64>().unwrap()
    }

    fn number(&mut self) {
        let value: Value = self.str_to_float(self.parser.previous.unwrap());
        self.emit_constant(value);
    }

    fn grouping(&mut self) {
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after expression");
    }

    fn unary(&mut self) {
        let operator_type = self.parser.previous.unwrap().token_type;

        // we put expression first because we would first evaluate the operand
        // then put in on stack then pop it and negate.
        self.expression();

        match operator_type {
            TokenType::Minus => self.emit_byte(OpCode::Negate as u8),
            _ => return,
        }
    }

    fn parse_precedence(&mut self, precedence: Precedence) {}
}
