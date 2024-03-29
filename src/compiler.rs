use crate::chunk::Chunk;
use crate::common::{FatPointer, Function, FunctionType, Obj, OpCode, Value};
use crate::hash_map::Table;
use crate::hasher;
use crate::memory;
use crate::scanner::{Scanner, Token, TokenType};
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
const OR: Option<ParseFn> = Some(|compiler, can_assign| compiler.or(can_assign));
const AND: Option<ParseFn> = Some(|compiler, can_assign| compiler.and(can_assign));
const CALL: Option<ParseFn> = Some(|compiler, can_assign| compiler.call(can_assign));

fn parse_rule(token_type: TokenType) -> ParseRule {
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

type ParseFn = fn(compiler: &mut Compiler, can_assign: bool);

struct ParseRule {
    prefix: Option<ParseFn>,
    infix: Option<ParseFn>,
    precedence: Precedence,
}

#[derive(Debug, Clone)]
struct Parser {
    current: Option<Token>,
    previous: Option<Token>,
    had_error: bool,
    panic_mode: bool,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum Local {
    Filled(Token, usize),
    Empty,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum UpValue {
    Filled(u8, bool),
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct CompilerContext {
    function: Obj,
    locals: Vec<Local>,
    local_count: usize,
    up_values: Vec<UpValue>,
    up_value_count: usize,
}

impl CompilerContext {
    fn init() -> CompilerContext {
        let mut locals = vec![];
        locals.resize(u8::MAX as usize, Local::Empty);

        let mut up_values = vec![];
        up_values.resize(u8::MAX as usize, UpValue::Empty);

        CompilerContext {
            locals,
            local_count: 1, // starting with 1 take first spot for top level function
            up_values,
            up_value_count: 0,
            function: Obj::Fun(Function::new_function(FunctionType::Script)),
        }
    }

    fn update_function_arity(&mut self, arity: u8) {
        if let Obj::Fun(function) = &mut self.function {
            function.arity = arity;
        }
    }
}

pub(crate) struct Compiler<'c> {
    table: &'c mut Table<Value>,
    scanner: Scanner,
    parser: Parser,
    source: String,
    current_context: usize,
    scope_depth: usize,
    contexts: Vec<CompilerContext>,
}

impl<'c> Compiler<'c> {
    pub(crate) fn init(scanner: Scanner, table: &'c mut Table<Value>) -> Compiler {
        let parser = Parser {
            current: None,
            previous: None,
            had_error: false,
            panic_mode: false,
        };

        let mut contexts: Vec<CompilerContext> = vec![];
        contexts.push(CompilerContext::init());

        let compiler = Compiler {
            scanner,
            parser,
            source: "".to_string(),
            table,
            contexts,
            scope_depth: 0,
            current_context: 0,
        };

        compiler
    }

    pub(crate) fn compile(&mut self, source: String) -> (bool, Obj) {
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
            self.current_context().function.clone(),
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
        if self.match_token(TokenType::Fun) {
            self.fun_decl();
        } else if self.match_token(TokenType::Var) {
            self.variable_decl();
        } else {
            self.statement();
        }

        if self.parser.panic_mode {
            self.synchronize_error();
        }
    }

    fn fun_decl(&mut self) {
        let index = self.parse_variable();
        let prev_token = self.parser.previous.unwrap();
        self.function();
        self.emit_opcode(OpCode::DefineGlobalVariable);
        self.current_chunk().write_index(index, prev_token.line);
    }

    fn function(&mut self) {
        let mut context = CompilerContext::init();
        let mut function = Function::new_function(FunctionType::Closure);
        let token = self.parser.previous.unwrap();
        let str_value = &self.source[token.start..token.start + token.length];
        let hash_value = hasher::hash(str_value);
        let exiting_value = self.table.find_entry_with_value(str_value, hash_value);
        function.name = exiting_value.cloned();
        let function_obj = Obj::Fun(function);
        context.function = function_obj;
        self.contexts.push(context);
        self.current_context += 1;
        self.begin_scope();
        self.consume(TokenType::LeftParen, "Expect '(' after function name");
        let mut arity = 0;
        //@todo parse arguments
        if !self.check(TokenType::RightParen) {
            self.parse_and_define_parameter();
            arity += 1;
            loop {
                match self.match_token(TokenType::Comma) {
                    true => {
                        self.parse_and_define_parameter();
                        arity += 1;
                    }
                    false => break,
                }
            }
        }

        if arity >= 255 {
            self.error_at_current("Can't have more than 255 parameters.");
        }

        self.current_context().update_function_arity(arity);
        self.consume(
            TokenType::RightParen,
            "Expect ')' at the end of function params",
        );
        self.consume(
            TokenType::LeftBrace,
            "Expect '{' at the beginning  of function body",
        );
        self.block();
        self.end_scope();
        self.end_compiler();
        let inner_function = self.contexts[self.current_context + 1].function.clone();
        let up_values = self.contexts[self.current_context + 1].up_values.clone();
        self.contexts.remove(self.current_context + 1);
        // reset old compiler state
        let constant_index = self
            .current_chunk()
            .add_constant(Value::from(inner_function));
        self.emit_opcode(OpCode::Closure);
        self.emit_byte(constant_index as u8);
        up_values.iter().for_each(|up_value| match up_value {
            UpValue::Filled(index, true) => {
                self.emit_byte(1);
                self.emit_byte(*index);
            }
            UpValue::Filled(index, false) => {
                self.emit_byte(0);
                self.emit_byte(*index);
            }
            _ => (),
        });
    }

    fn recursive_resolve_up_value(
        &mut self,
        name: Token,
        context_index: usize,
        scope_depth: usize,
    ) -> i32 {
        /*
            let's say we have this:
            ```
                fun outer_1() {
                    let x = "outer_1";
                    fun inner_1() {
                        let y = "inner_1";
                        fun inner_2() {
                            print x;
                            print y;
                        }
                    }
                }
            ```

            in this case when we are in inner_2, our context index is 4 (including index of main function)
            so this logic will first try to find x in using locals from context index 3 which is inner_1 function.
            it will search in those locals but it doesn't exist so recursively it will call for index 3.
            then same logic will be applied and it will search x in index 2 which is our outer_1 function. x exists there
            so we will get a valid index. Then index 3 call will add a upvalue in its compiler context
            and return index. which will be received by first call using context index 4 and it will also add
            add local value using false.
        */
        if context_index == 0 {
            return -1;
        }

        let context_opt = self.contexts.get(context_index - 1);
        match context_opt {
            Some(context) => {
                let locals = context.locals.clone();
                if let Some(index) = self.resolve_from_locals(locals, scope_depth - 1, name) {
                    self.add_up_value(index as u8, true, context_index);
                    return index;
                } else {
                    let r_index =
                        self.recursive_resolve_up_value(name, context_index - 1, scope_depth - 1);
                    self.add_up_value(r_index as u8, false, context_index);
                    r_index
                }
            }
            None => -1,
        }
    }

    fn add_up_value(&mut self, index: u8, is_local: bool, context_index: usize) {
        let up_value = UpValue::Filled(index, is_local);
        let up_value_count = self.contexts[context_index].up_value_count;
        self.contexts[context_index].up_values[up_value_count] = up_value;
        self.contexts[context_index].up_value_count += 1;
    }

    fn parse_and_define_parameter(&mut self) {
        let param_index = self.parse_variable();
        self.define_variable(param_index);
    }

    fn variable_decl(&mut self) {
        let index = self.parse_variable();
        if self.match_token(TokenType::Equal) {
            self.expression();
        } else {
            self.emit_opcode(OpCode::Nil);
        }

        self.consume_semicolon();
        self.define_variable(index)
    }

    fn parse_variable(&mut self) -> usize {
        self.consume(TokenType::Identifier, "Expected name after variable");
        self.declare_variable();
        let mut index = 0;
        if self.scope_depth <= 0 {
            index = self.identifier();
        }
        index
    }

    fn declare_variable(&mut self) {
        if self.scope_depth > 0 {
            if self.current_context().local_count == 255 {
                self.error("Too many local variables in function.");
                return;
            }
            let token = self.parser.previous.unwrap();
            let local = Local::Filled(token, self.scope_depth);

            let matching_token = self.resolve_local(token);

            if matching_token != -1 {
                self.error("Already a variable with this name in this scope.");
            }
            let local_count = self.current_context().local_count;
            self.current_context().locals[local_count] = local;
            self.current_context().local_count += 1;
        }
    }

    fn resolve_local(&mut self, token: Token) -> i32 {
        if self.current_context().local_count <= 0 {
            return -1;
        }
        let scope_depth = self.scope_depth;
        let locals = self.current_context().locals.clone();

        if let Some(value) = self.resolve_from_locals(locals, scope_depth, token) {
            return value;
        }
        -1
    }

    fn resolve_from_locals(
        &mut self,
        locals: Vec<Local>,
        scope_depth: usize,
        token: Token,
    ) -> Option<i32> {
        for (idx, existing) in locals.iter().enumerate().rev() {
            match existing {
                Local::Filled(existing_token, depth) => {
                    if depth.ge(&scope_depth) {
                        if token.length != existing_token.length {
                            continue;
                        }
                        let existing_token_deref = existing_token.clone();
                        let existing_name = self.token_name(existing_token_deref);
                        let local_name = self.token_name(token);
                        if local_name == existing_name {
                            return Some(idx as i32);
                        }
                    }
                }
                _ => continue,
            }
        }
        None
    }

    fn variable(&mut self, can_assign: bool) {
        let token = self.parser.previous.unwrap();
        let mut existing_index = self.resolve_local(token);
        let mut set_op = OpCode::Nil;
        let mut get_op = OpCode::Nil;
        if existing_index >= 0 {
            set_op = OpCode::SetLocalVariable;
            get_op = OpCode::GetLocalVariable;
        } else {
            existing_index =
                self.recursive_resolve_up_value(token, self.current_context, self.scope_depth);
            if existing_index != -1 {
                set_op = OpCode::SetUpValue;
                get_op = OpCode::GetUpValue;
            } else {
                let index = self.identifier();
                existing_index = index as i32;
                set_op = OpCode::SetGlobalVariable;
                get_op = OpCode::GetGlobalVariable;
            }
        }
        let prev_token = self.previous_token();
        if can_assign && self.match_token(TokenType::Equal) {
            self.expression();
            self.emit_opcode(set_op);
            self.current_chunk()
                // @type_conversion this conversion here to usize will result in usize::MAX
                // when existing_index is -1
                .write_index(existing_index as usize, prev_token.line);
        } else {
            self.emit_opcode(get_op);
            self.current_chunk()
                // @type_conversion this conversion here to usize will result in usize::MAX
                // when existing_index is -1
                .write_index(existing_index as usize, prev_token.line);
        }
    }

    fn define_variable(&mut self, index: usize) {
        if self.scope_depth > 0 {
            return;
        }
        let prev_token = self.previous_token();
        self.emit_opcode(OpCode::DefineGlobalVariable);
        self.current_chunk().write_index(index, prev_token.line);
    }

    fn identifier(&mut self) -> usize {
        self.string(false, false)
    }

    fn statement(&mut self) {
        if self.match_token(TokenType::Print) {
            self.print_stmt();
        } else if self.match_token(TokenType::If) {
            self.if_stmt();
        } else if self.match_token(TokenType::Return) {
            self.return_stmt();
        } else if self.match_token(TokenType::While) {
            self.while_stmt();
        } else if self.match_token(TokenType::For) {
            self.for_stmt();
        } else if self.match_token(TokenType::LeftBrace) {
            self.begin_scope();
            self.block();
            self.end_scope();
        } else {
            self.expression_statement();
        }
    }

    fn return_stmt(&mut self) {
        if self.match_token(TokenType::Semicolon) {
            self.emit_return();
        } else {
            self.expression();
            self.consume_semicolon();
            self.emit_opcode(OpCode::Return);
        }
    }

    fn for_stmt(&mut self) {
        self.begin_scope();
        self.consume(TokenType::LeftParen, "Expect '(' after if statement");

        // optional init
        if !self.match_token(TokenType::Semicolon) {
            if self.match_token(TokenType::Var) {
                self.variable_decl();
            } else {
                self.expression_statement();
            }
        }
        // loop always comes back to condition after increment if there is increment
        // loop_start will move to inc_start if increment expression is available.
        let mut loop_start = self.current_chunk().code.len();

        // optional condition
        let mut end_loop = usize::MAX;
        if !self.match_token(TokenType::Semicolon) {
            self.expression();
            self.consume_semicolon();
            end_loop = self.emit_jump(OpCode::JumpIfFalse);
            self.emit_opcode(OpCode::Pop) // pop truthy
        }

        // optional increment block
        if !self.match_token(TokenType::RightParen) {
            let body_jump = self.emit_jump(OpCode::Jump);
            let inc_start = self.current_chunk().code.len();
            self.expression();
            self.emit_opcode(OpCode::Pop);
            self.consume(
                TokenType::RightParen,
                "Expect ')' at the end of if statement",
            );
            self.emit_loop(loop_start);
            loop_start = inc_start;
            self.patch_jump(body_jump);
        }

        self.statement();
        self.emit_loop(loop_start);
        // jump to end of loop if condition is false but
        // only if there is a condition as its optional.
        if end_loop != usize::MAX {
            self.patch_jump(end_loop);
            self.emit_opcode(OpCode::Pop) // pop false
        }

        self.end_scope();
    }

    fn while_stmt(&mut self) {
        let loop_start = self.current_chunk().code.len();
        self.consume(TokenType::LeftParen, "Expect '(' after if statement");
        self.expression();
        self.consume(
            TokenType::RightParen,
            "Expect ')' at the end of if statement",
        );
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_opcode(OpCode::Pop); // remove truthy result
        self.statement();
        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);
        self.emit_opcode(OpCode::Pop); // remove falsey result
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_opcode(OpCode::Loop);
        let jump = (self.current_chunk().code.len() - loop_start + 2) as u16;

        if jump > u16::MAX {
            self.error(format!("Can not jump more than {:?} bytes", u16::MAX).as_str());
        } else {
            self.emit_byte(((jump >> 8) & 0xff) as u8);
            self.emit_byte((jump & 0xff) as u8);
        }
    }

    fn if_stmt(&mut self) {
        self.consume(TokenType::LeftParen, "Expect '(' after if statement");
        self.expression();
        self.consume(
            TokenType::RightParen,
            "Expect ')' at the end of if statement",
        );
        let offset = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_opcode(OpCode::Pop); // remove if condition result from stack top when if is truthy
        self.statement();
        let else_offset = self.emit_jump(OpCode::Jump);
        self.patch_jump(offset);
        self.emit_opcode(OpCode::Pop); // remove if condition result from stack top when if is not truthy
        if self.match_token(TokenType::Else) {
            self.statement();
        }
        self.patch_jump(else_offset);
    }

    fn emit_jump(&mut self, instruction: OpCode) -> usize {
        self.emit_opcode(instruction);
        //We use two bytes for the jump offset operand.
        //A 16-bit offset lets us jump over up to 65,535 bytes of code,
        // which should be plenty for our needs.
        self.emit_byte(0xff);
        self.emit_byte(0xff);
        // return index to where we emit two bytes for offset operand.
        self.current_chunk().code.len() - 2
    }

    fn patch_jump(&mut self, offset: usize) {
        // if we start emitting jump_if_else when ip is set to 5
        // jump_if_else will go at 6, first 8 bits of offset to 7 and last 8 bits
        // to 8th index of ip. Offset returned from emit_jump will be 6
        // as it removes the offset bytes. Lets assume we push 4 instructions as part of
        // if block. We calculate how much to jump if if condition is false.
        // 12 - 6 - 2 = 4, we need to skip 4 bytes which makes sense because
        // we did insert 4 instructions as part of if block.
        // -2 to adjust for the bytecode for the jump offset itself.
        let jump = (self.current_chunk().code.len() - offset - 2) as u16;

        if jump > u16::MAX {
            self.error(format!("Can not jump more than {:?} bytes", u16::MAX).as_str());
        } else {
            // get msb 8 bits from the offset and mask with 0xff to
            // make other bits are reset
            self.current_chunk().code[offset] = ((jump >> 8) & 0xff) as u8;
            // get lsb 8 bits
            self.current_chunk().code[offset + 1] = (jump & 0xff) as u8;
        }
    }

    fn block(&mut self) {
        while !self.check(TokenType::RightBrace) && !self.check(TokenType::Eof) {
            self.declaration();
        }

        self.consume(TokenType::RightBrace, "Expect '}' after block.");
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        let scoped_locals = self
            .current_context()
            .locals
            .clone()
            .iter()
            .filter(|local| match local {
                Local::Filled(_, depth) => depth.gt(&self.scope_depth),
                _ => false,
            })
            .count();
        if self.current_context().local_count == 0 {
            return;
        }
        for _ in 1..=scoped_locals {
            self.emit_opcode(OpCode::Pop);
        }
        self.current_context().local_count -= scoped_locals;
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
        let prev_token = self.previous_token();
        self.current_chunk().write_chunk(byte, prev_token.line);
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
        // do it only for inner functions
        if self.current_context > 0 {
            self.current_context -= 1;
        }
    }

    fn emit_return(&mut self) {
        self.emit_opcode(OpCode::Nil);
        self.emit_opcode(OpCode::Return);
    }

    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    fn emit_constant(&mut self, value: Value) -> usize {
        let prev_token = self.previous_token();
        self.current_chunk().write_constant(value, prev_token.line)
    }

    fn str_to_float(&mut self, token: Token) -> f64 {
        let value = self.token_name(token);
        value.parse::<f64>().unwrap()
    }

    fn number(&mut self, _can_assign: bool) {
        let value: f64 = self.str_to_float(self.parser.previous.unwrap());
        self.emit_constant(Value::from(value));
    }

    fn and(&mut self, _can_assign: bool) {
        let offset = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_opcode(OpCode::Pop);
        self.parse_precedence(Precedence::And);
        self.patch_jump(offset);
    }

    fn or(&mut self, _can_assign: bool) {
        let else_jump_offset = self.emit_jump(OpCode::JumpIfFalse);
        let end_jump_offset = self.emit_jump(OpCode::Jump);
        self.patch_jump(else_jump_offset);
        self.emit_opcode(OpCode::Pop);
        self.parse_precedence(Precedence::Or);
        self.patch_jump(end_jump_offset);
    }

    fn call(&mut self, _can_assign: bool) {
        let mut arg_count = 0;
        if !self.check(TokenType::RightParen) {
            self.expression();
            arg_count += 1;
            loop {
                match self.match_token(TokenType::Comma) {
                    true => {
                        self.expression();
                        arg_count += 1;
                        if arg_count == 255 {
                            self.error("Can't have more than 255 arguments");
                        }
                    }
                    false => break,
                }
            }
        }
        self.consume(TokenType::RightParen, "Expected ')' in function call.");
        self.emit_opcode(OpCode::Call);
        self.emit_byte(arg_count);
    }

    fn string(&mut self, _can_assign: bool, emit_constant: bool) -> usize {
        let (str_value, hash_value) = self.prev_token_to_string();
        let exiting_value = self.get_existing_string(&str_value, hash_value);
        match exiting_value {
            Some(existing) => {
                let existing_ptr = existing.to_owned();
                self.reuse_existing_string(existing_ptr, emit_constant)
            }
            None => self.create_new_string(str_value, hash_value, emit_constant),
        }
    }

    fn reuse_existing_string(&mut self, existing: FatPointer, emit_constant: bool) -> usize {
        let obj_string = Obj::from(existing);
        let value = Value::from(obj_string);
        if emit_constant {
            self.emit_constant(value)
        } else {
            self.current_chunk().add_constant(value)
        }
    }

    fn create_new_string(
        &mut self,
        mut str_value: String,
        hash_value: u32,
        emit_constant: bool,
    ) -> usize {
        let str_ptr = memory::allocate::<String>();
        let src = str_value.as_mut_ptr();
        memory::copy(src, str_ptr, str_value.len(), 0);
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
            self.current_chunk().add_constant(value)
        }
    }

    fn get_existing_string(&mut self, str_value: &str, hash_value: u32) -> Option<&FatPointer> {
        let exiting_value = self.table.find_entry_with_value(str_value, hash_value);
        exiting_value
    }

    fn prev_token_to_string(&mut self) -> (String, u32) {
        let token = self.parser.previous.unwrap();
        let str_value = self.token_name(token).to_owned();
        let hash_value = hasher::hash(&str_value);
        (str_value, hash_value)
    }

    fn grouping(&mut self, _can_assign: bool) {
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after expression");
    }

    fn unary(&mut self, _can_assign: bool) {
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

    fn binary(&mut self, _can_assign: bool) {
        let operator_type = self.parser.previous.unwrap().token_type;
        let rule = self.get_rule(operator_type);
        let next_op: Precedence = num::FromPrimitive::from_u8((rule.precedence) as u8 + 1).unwrap();
        self.parse_precedence(next_op);
        self.emit_operator(operator_type);
    }

    fn literal(&mut self, _can_assign: bool) {
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

    fn current_chunk(&mut self) -> &mut Chunk {
        self.current_context().function.get_func_chunk()
    }

    fn current_context(&mut self) -> &mut CompilerContext {
        self.contexts.get_mut(self.current_context).unwrap()
    }

    fn previous_token(&self) -> Token {
        self.parser.previous.unwrap()
    }
}
