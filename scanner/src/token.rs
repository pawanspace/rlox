use num_derive::FromPrimitive;
use std::cmp::Ordering;

#[derive(Debug, PartialEq, Copy, Clone, FromPrimitive, Hash, Eq)]
pub enum TokenType {
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
pub struct Token {
    pub token_type: TokenType,
    pub start: usize,
    pub length: usize,
    pub line: u32,
}
