use crate::chunk::Chunk;
use crate::common::{FatPointer, Function, FunctionType, Obj, OpCode, Value};
use crate::hash_map::Table;
use crate::hasher;
use crate::memory;
use crate::scanner::{Scanner, Token, TokenType};
use num_derive::FromPrimitive;
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

type ParseFn = fn(compiler: &mut Compiler, can_assign: bool);

struct ParseRule {
    prefix: Option<ParseFn>,
    infix: Option<ParseFn>,
    precedence: Precedence,
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
const OR: Option<ParseFn> = Some(|compiler, can_assign| compiler.or(can_assign));
const AND: Option<ParseFn> = Some(|compiler, can_assign| compiler.and(can_assign));
const CALL: Option<ParseFn> = Some(|compiler, can_assign| compiler.call(can_assign));



pub fn get_parse_rule(token_type: TokenType) -> ParseRule {
    match token_type {
        TokenType::LeftParen => ParseRule {
            prefix: GROUPING,
            infix: CALL,
            precedence: Precedence::Call,
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
        TokenType::Or => ParseRule {
            prefix: NOOP,
            infix: OR,
            precedence: Precedence::Or,
        },
        TokenType::And => ParseRule {
            prefix: NOOP,
            infix: AND,
            precedence: Precedence::And,
        },
        TokenType::Comma
        | TokenType::Class
        | TokenType::Else
        | TokenType::For
        | TokenType::Fun
        | TokenType::If
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