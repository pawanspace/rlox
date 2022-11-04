use std::mem;
use std::str::Chars;
use crate::memory;
use crate::chunk::Chunk;
use crate::common::{FatPointer, Obj, OpCode, Value};
use crate::scanner::{Scanner, Token, TokenType};
use crate::value::ValueArray;
use num_derive::FromPrimitive;

extern crate num;
// precedence level lower to higher
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[derive(FromPrimitive, Eq, PartialEq)]
enum Precedence {
    None = 1,
    Assignment = 2,
    Or = 3,
    And = 4,
    Equality = 5,
    Comparison = 6,
    Term = 7,
    Factor = 8,
    Unary = 9,
    Call = 10,
    Primary = 11,
}

const noop: Option<ParseFn> = None;
const grouping: Option<ParseFn> = Some(|compiler| compiler.grouping());
const binary: Option<ParseFn> = Some(|compiler| compiler.binary());
const unary: Option<ParseFn> = Some(|compiler| compiler.unary());
const number: Option<ParseFn> = Some(|compiler| compiler.number());
const literal: Option<ParseFn> = Some(|compiler| compiler.literal());
const string: Option<ParseFn> = Some(|compiler| compiler.string());

fn parse_rule(token_type: TokenType) -> ParseRule {
    match token_type {
        TokenType::LeftParen => ParseRule {
            prefix: grouping,
            infix: noop,
            precedence: Precedence::None,
        },
        TokenType::Minus | TokenType::Bang => ParseRule {
            prefix: unary,
            infix: binary,
            precedence: Precedence::Term,
        },
        TokenType::Plus => ParseRule {
            prefix: noop,
            infix: binary,
            precedence: Precedence::Term,
        },
        TokenType::EqualEqual | TokenType::BangEqual => ParseRule {
            prefix: noop,
            infix: binary,
            precedence: Precedence::Equality,
        },
        TokenType::Greater | TokenType::Less | TokenType::GreaterEqual | TokenType::LessEqual => {
            ParseRule {
                prefix: noop,
                infix: binary,
                precedence: Precedence::Comparison,
            }
        }
        TokenType::Star | TokenType::Slash => ParseRule {
            prefix: noop,
            infix: binary,
            precedence: Precedence::Factor,
        },
        TokenType::Number => ParseRule {
            prefix: number,
            infix: noop,
            precedence: Precedence::None,
        },
        TokenType::False | TokenType::True | TokenType::Nil => ParseRule {
            prefix: literal,
            infix: noop,
            precedence: Precedence::None,
        },
        TokenType::String => ParseRule {
            prefix: string,
            infix: noop,
            precedence: Precedence::None,
        },
        TokenType::Comma
        | TokenType::And
        | TokenType::Class
        | TokenType::Else
        | TokenType::For
        | TokenType::Fun
        | TokenType::If
        | TokenType::Or
        | TokenType::Print
        | TokenType::Return
        | TokenType::Super
        | TokenType::This
        | TokenType::Var
        | TokenType::While
        | TokenType::Error
        | TokenType::Eof
        | TokenType::Semicolon
        | TokenType::Equal
        | TokenType::Identifier
        | TokenType::String
        | TokenType::Dot
        | TokenType::LeftBrace
        | TokenType::RightBrace
        | TokenType::RightParen
        | _ => ParseRule {
            prefix: noop,
            infix: noop,
            precedence: Precedence::None,
        },
    }
}

type ParseFn = fn(compiler: &mut Compiler);

struct ParseRule {
    prefix: Option<ParseFn>,
    infix: Option<ParseFn>,
    precedence: Precedence,
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
        self.expression();
        self.consume(TokenType::Eof, "Expect end of expression.");
        self.end_compiler();
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

            self.error_at_current("@todo some error here")
        }
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

    fn emit_opcode(&mut self, opcode: OpCode) {
        self.emit_byte(opcode as u8);
    }

    fn emit_opcodes(&mut self, opcode_1: OpCode, opcode_2: OpCode) {
        self.emit_opcode(opcode_1);
        self.emit_opcode(opcode_2);
    }

    fn end_compiler(&mut self) {
        self.emit_return();
        //self.chunk.disassemble_chunk("Compiler");
    }

    fn emit_return(&mut self) {
        self.emit_opcode(OpCode::Return)
    }

    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    fn emit_constant(&mut self, value: Value) {
        self.chunk
            .write_constant(value, self.parser.previous.unwrap().line)
    }

    fn str_to_float(&mut self, token: Token) -> f64 {
        let value = &self.source[token.start..token.start + token.length];
        value.parse::<f64>().unwrap()
    }

    fn number(&mut self) {
        let value: f64 = self.str_to_float(self.parser.previous.unwrap());
        self.emit_constant(Value::from(value));
    }

    fn string(&mut self) {
        let mut token = self.parser.previous.unwrap();
        let mut str_value = &mut self.source[token.start..token.start + token.length];

        let str_ptr = memory::allocate::<String>();
        memory::copy(str_value.as_mut_ptr(), str_ptr, str_value.len(), 0);

        let obj_string =  Obj::from(FatPointer {ptr: str_ptr, size: str_value.len() });
        let value = Value::from(obj_string);
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
        self.parse_precedence(Precedence::Unary);

        match operator_type {
            TokenType::Minus => self.emit_opcode(OpCode::Negate),
            TokenType::Bang => self.emit_opcode(OpCode::Not),
            _ => return,
        }
    }

    fn emit_operator(&mut self, operator_type: TokenType) {
        match operator_type {
            TokenType::Minus => self.emit_opcode(OpCode::Subtract),
            TokenType::Plus => self.emit_opcode(OpCode::Add),
            TokenType::Star => self.emit_opcode(OpCode::Multiply),
            TokenType::Slash => self.emit_opcode(OpCode::Divide),
            TokenType::Greater => self.emit_opcode(OpCode::Greater),
            TokenType::GreaterEqual => self.emit_opcodes(OpCode::Less, OpCode::Not),
            TokenType::Less => self.emit_opcode(OpCode::Less),
            TokenType::LessEqual => self.emit_opcodes(OpCode::Greater, OpCode::Not),
            TokenType::EqualEqual => self.emit_opcode(OpCode::Equal),
            TokenType::BangEqual => self.emit_opcodes(OpCode::Equal, OpCode::Not),

            _ => return,
        }
    }

    fn get_rule(&mut self, token_type: TokenType) -> ParseRule {
        parse_rule(token_type)
    }

    fn binary(&mut self) {
        let operator_type = self.parser.previous.unwrap().token_type;
        let rule = self.get_rule(operator_type);
        let next_op: Precedence = num::FromPrimitive::from_u8((rule.precedence) as u8 + 1).unwrap();
        self.parse_precedence(next_op);
        self.emit_operator(operator_type);
    }

    fn literal(&mut self) {
        let token_type = self.parser.previous.unwrap().token_type;
        match token_type {
            TokenType::False => self.emit_opcode(OpCode::False),
            TokenType::Nil => self.emit_opcode(OpCode::Nil),
            TokenType::True => self.emit_opcode(OpCode::True),
            _ => println!("Unknown type: {:?} ", token_type),
        }
    }

    fn parse_precedence(&mut self, precedence: Precedence) {
        self.advance();
        let prefix = self
            .get_rule(self.parser.previous.unwrap().token_type)
            .prefix;

        if prefix.is_none() {
            self.error("Expect expression");
            return;
        }

        let prefix_func = prefix.unwrap();
        prefix_func(self);

        while precedence as u8
            <= self
                .get_rule(self.parser.current.unwrap().token_type)
                .precedence as u8
        {
            self.advance();
            let infix = self
                .get_rule(self.parser.previous.unwrap().token_type)
                .infix;
            let infix_func = infix.unwrap();
            infix_func(self);
        }
    }
}
