use crate::chunk::Chunk;
use crate::common::{FatPointer, Obj, OpCode, Value};
use crate::hash_map::Table;
use crate::hasher;
use crate::memory;
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

const NOOP: Option<ParseFn> = None;
const GROUPING: Option<ParseFn> = Some(|compiler, can_assign| compiler.grouping(can_assign));
const BINARY: Option<ParseFn> = Some(|compiler, can_assign| compiler.binary(can_assign));
const UNARY: Option<ParseFn> = Some(|compiler, can_assign| compiler.unary(can_assign));
const NUMBER: Option<ParseFn> = Some(|compiler, can_assign| compiler.number(can_assign));
const LITERAL: Option<ParseFn> = Some(|compiler, can_assign| compiler.literal(can_assign));
const STRING: Option<ParseFn> = Some(|compiler, can_assign| {
    compiler.string(can_assign, true);
});
const VARIABLE: Option<ParseFn> = Some(|compiler, can_assign| compiler.variable(can_assign));

fn parse_rule(token_type: TokenType) -> ParseRule {
    match token_type {
        TokenType::LeftParen => ParseRule {
            prefix: GROUPING,
            infix: NOOP,
            precedence: Precedence::None,
        },
        TokenType::Minus | TokenType::Bang => ParseRule {
            prefix: UNARY,
            infix: BINARY,
            precedence: Precedence::Term,
        },
        TokenType::Plus => ParseRule {
            prefix: NOOP,
            infix: BINARY,
            precedence: Precedence::Term,
        },
        TokenType::EqualEqual | TokenType::BangEqual => ParseRule {
            prefix: NOOP,
            infix: BINARY,
            precedence: Precedence::Equality,
        },
        TokenType::Greater | TokenType::Less | TokenType::GreaterEqual | TokenType::LessEqual => {
            ParseRule {
                prefix: NOOP,
                infix: BINARY,
                precedence: Precedence::Comparison,
            }
        }
        TokenType::Star | TokenType::Slash => ParseRule {
            prefix: NOOP,
            infix: BINARY,
            precedence: Precedence::Factor,
        },
        TokenType::Number => ParseRule {
            prefix: NUMBER,
            infix: NOOP,
            precedence: Precedence::None,
        },
        TokenType::False | TokenType::True | TokenType::Nil => ParseRule {
            prefix: LITERAL,
            infix: NOOP,
            precedence: Precedence::None,
        },
        TokenType::String => ParseRule {
            prefix: STRING,
            infix: NOOP,
            precedence: Precedence::None,
        },
        TokenType::Identifier => ParseRule {
            prefix: VARIABLE,
            infix: NOOP,
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
        | TokenType::Dot
        | TokenType::LeftBrace
        | TokenType::RightBrace
        | TokenType::RightParen
        | _ => ParseRule {
            prefix: NOOP,
            infix: NOOP,
            precedence: Precedence::None,
        },
    }
}

type ParseFn = fn(compiler: &mut Compiler, can_assign: bool);

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
#[derive(Debug, Clone, Copy)]
pub(crate) struct Local {
    name: Token,
    depth: usize,
}

pub(crate) struct Compiler<'c> {
    scanner: Scanner,
    parser: Parser,
    chunk: Box<Chunk>,
    source: String,
    table: &'c mut Table<Value>,
    locals: Vec<Local>,
    local_count: usize,
    scope_depth: usize,
}

impl<'c> Compiler<'c> {
    pub(crate) fn init(
        scanner: Scanner,
        chunk: Box<Chunk>,
        table: &'c mut Table<Value>,
    ) -> Compiler {
        let parser = Parser {
            current: None,
            previous: None,
            had_error: false,
            panic_mode: false,
        };

        let locals = vec![];

        Compiler {
            scanner,
            parser,
            chunk,
            source: "".to_string(),
            table: table,
            locals,
            local_count: 0,
            scope_depth: 0,
        }
    }

    pub(crate) fn compile(&mut self, source: String) -> (bool, Box<Chunk>) {
        self.source = source;
        let chars: Vec<char> = self.source.chars().collect();
        self.scanner.refresh(0, self.source.len(), chars);
        self.advance();
        while !self.match_token(TokenType::Eof) {
            self.declaration();
        }
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

    fn declaration(&mut self) {
        if self.match_token(TokenType::Var) {
            self.variable_decl();
        } else {
            self.statement();
        }

        if self.parser.panic_mode {
            self.synchronize_error();
        }
    }

    fn variable_decl(&mut self) {
        self.consume(TokenType::Identifier, "Expected name after variable");
        self.declare_variable();

        let mut index = 0;
        if self.scope_depth <= 0 {
            index = self.identifier();
        }

        if self.match_token(TokenType::Equal) {
            self.expression();
        } else {
            self.emit_opcode(OpCode::Nil);
        }

        self.consume_semicolon();
        self.define_variable(index)
    }

    fn declare_variable(&mut self) {
        if self.scope_depth > 0 {
            if self.local_count == 255 {
                self.error("Too many local variables in function.");
                return;
            }

            let local = Local {
                name: self.parser.previous.unwrap(),
                depth: self.scope_depth,
            };

            let matching_token = self.resolve_local(local.name);

            if matching_token != -1 {
                self.error("Already a variable with this name in this scope.");
            }

            self.locals[self.local_count] = local;
            self.local_count += 1;
        }
    }

    fn resolve_local(&mut self, token: Token) -> i32 {
        let mut index = -1;
        for (idx, existing) in self.locals.iter().enumerate() {
            if existing.depth >= self.scope_depth {
                let existing_token = existing.name;

                if token.length != existing_token.length {
                    continue;
                }

                let existing_name = self.token_name(existing_token);
                let local_name = self.token_name(token);
                if local_name == existing_name {
                    index = idx as i32;
                }
            }
        }
        index
    }

    fn variable(&mut self, can_assign: bool) {
        let token = self.parser.previous.unwrap();
        let mut existing_index = self.resolve_local(token);
        let mut set_op = OpCode::Nil;
        let mut get_op  = OpCode::Nil;
        if existing_index > 0 {
            set_op = OpCode::SetLocalVariable;
            get_op = OpCode::GetLocalVariable;
        } else {
            let index = self.identifier();
            existing_index = index as i32;
            set_op = OpCode::SetGlobalVariable;
            get_op = OpCode::GetGloablVariable;
        }

        if can_assign && self.match_token(TokenType::Equal) {
            self.expression();
            self.emit_opcode(set_op);
            self.chunk
            // @type_conversion this conversion here to uszie will result in uusize::MAX
            // when existing_index is -1
            .write_index(existing_index as usize, self.parser.previous.unwrap().line);
        } else {
            self.emit_opcode(get_op);
        }
    }

    fn define_variable(&mut self, index: usize) {
        if self.scope_depth > 0 {
            return;
        }

        self.emit_opcode(OpCode::DefineGlobalVariable);
        self.chunk
            .write_index(index, self.parser.previous.unwrap().line);
    }

    fn identifier(&mut self) -> usize {
        self.string(false, false)
    }

    fn statement(&mut self) {
        if self.match_token(TokenType::Print) {
            self.print_stmt();
        } else if self.match_token(TokenType::LeftBrace) {
            self.begin_scope();
            self.block();
            self.end_scope();
        } else {
            self.expression_statement();
        }
    }

    fn block(&mut self) {
        while self.check(TokenType::RightBrace) || self.check(TokenType::Eof) {
            self.declaration();
        }

        self.consume(TokenType::RightBrace, "Expect '(' after block.");
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        let scoped_locals = self
            .locals
            .iter()
            .filter(|local| local.depth > self.scope_depth)
            .count();

        for _ in 1..=scoped_locals {
            self.emit_opcode(OpCode::Pop);
        }
        self.local_count -= scoped_locals;
    }

    fn expression_statement(&mut self) {
        self.expression();
        self.consume_semicolon();
        self.emit_opcode(OpCode::Pop);
    }

    fn consume_semicolon(&mut self) {
        self.consume(
            TokenType::Semicolon,
            "Expected semicolon at the end of experession",
        );
    }

    fn synchronize_error(&mut self) {
        self.parser.panic_mode = false;

        while !self.check(TokenType::Eof) {
            if self.check(TokenType::Semicolon) {
                return;
            }

            match self.parser.current.unwrap().token_type {
                TokenType::Class
                | TokenType::Fun
                | TokenType::If
                | TokenType::While
                | TokenType::Var
                | TokenType::Print
                | TokenType::For
                | TokenType::Return => return,
                _ => self.advance(),
            }
        }
    }

    fn print_stmt(&mut self) {
        self.expression();
        self.consume_semicolon();
        self.emit_opcode(OpCode::Print);
    }

    fn match_token(&mut self, token_type: TokenType) -> bool {
        if self.check(token_type) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, token_type: TokenType) -> bool {
        match self.parser.current {
            Some(current_token) => current_token.token_type == token_type,
            None => false,
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

    fn emit_constant(&mut self, value: Value) -> usize {
        self.chunk
            .write_constant(value, self.parser.previous.unwrap().line)
    }

    fn str_to_float(&mut self, token: Token) -> f64 {
        let value = self.token_name(token);
        value.parse::<f64>().unwrap()
    }

    fn number(&mut self, can_assign: bool) {
        let value: f64 = self.str_to_float(self.parser.previous.unwrap());
        self.emit_constant(Value::from(value));
    }

    fn string(&mut self, can_assign: bool, emit_constant: bool) -> usize {
        let token = self.parser.previous.unwrap();
        let str_value = &mut self.source[token.start..token.start + token.length];
        let hash_value = hasher::hash(str_value);

        let exiting_value = self.table.find_entry_with_value(str_value, hash_value);

        match exiting_value {
            Some(existing) => {
                let obj_string = Obj::from(existing.clone());
                let value = Value::from(obj_string);
                if emit_constant {
                    self.emit_constant(value)
                } else {
                    self.chunk.add_constant(value)
                }
            }
            None => {
                let str_ptr = memory::allocate::<String>();
                memory::copy(str_value.as_mut_ptr(), str_ptr, str_value.len(), 0);
                let fat_ptr = FatPointer {
                    ptr: str_ptr,
                    size: str_value.len(),
                    hash: hash_value,
                };
                let obj_string = Obj::from(fat_ptr.clone());
                let value = Value::from(obj_string);
                self.table.insert(fat_ptr.clone(), Value::Missing);
                if emit_constant {
                    self.emit_constant(value)
                } else {
                    self.chunk.add_constant(value)
                }
            }
        }
    }

    fn grouping(&mut self, can_assign: bool) {
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after expression");
    }

    fn unary(&mut self, can_assign: bool) {
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

    fn binary(&mut self, can_assign: bool) {
        let operator_type = self.parser.previous.unwrap().token_type;
        let rule = self.get_rule(operator_type);
        let next_op: Precedence = num::FromPrimitive::from_u8((rule.precedence) as u8 + 1).unwrap();
        self.parse_precedence(next_op);
        self.emit_operator(operator_type);
    }

    fn literal(&mut self, can_assign: bool) {
        let token_type = self.parser.previous.unwrap().token_type;
        match token_type {
            TokenType::False => self.emit_opcode(OpCode::False),
            TokenType::Nil => self.emit_opcode(OpCode::Nil),
            TokenType::True => self.emit_opcode(OpCode::True),
            _ => println!("Unknown type: {:?} ", token_type),
        }
    }

    fn token_name(&self, token: Token) -> &str {
        &self.source[token.start..token.start + token.length]
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

        let can_assign = precedence as u8 <= Precedence::Assignment as u8;

        let prefix_func = prefix.unwrap();
        prefix_func(self, can_assign);

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
            infix_func(self, can_assign);
        }
        if can_assign && self.match_token(TokenType::Equal) {
            self.error("Invalid assignment target");
        }
    }
}
