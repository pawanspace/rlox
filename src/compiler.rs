//! # The Compiler (single-pass bytecode compiler + Pratt parser)
//!
//! This file is the middle of the pipeline:
//!
//! ```text
//!   source text  ->  Scanner  ->  Compiler  ->  Chunk (bytecode)  ->  VM
//!                     (tokens)     (this file)   (byte array)         (executes)
//! ```
//!
//! ## Single-pass compilation
//! Unlike a classic compiler that first builds an Abstract Syntax Tree (AST)
//! and then walks it to generate code, this compiler is *single-pass*: it
//! reads tokens one at a time and emits bytecode *immediately* as it parses.
//! There is no tree in memory. This is the design used by clox (the C
//! implementation in the book "Crafting Interpreters"), and it is fast and
//! memory-light, but it means the compiler must sometimes emit an instruction
//! before it knows a value it needs (see "backpatching" in `patch_jump`).
//!
//! ## Pratt parsing (a.k.a. precedence climbing)
//! Expressions are parsed with a *Pratt parser*. The core idea: every token
//! type is associated with up to two handlers and a binding power
//! (precedence):
//!   - a *prefix* handler — used when the token appears at the start of an
//!     expression (e.g. `-` in `-x`, a number literal, `(` for grouping).
//!   - an *infix* handler — used when the token appears *between* two
//!     operands (e.g. `+` in `a + b`).
//! `parse_precedence` drives the loop: parse a prefix, then keep consuming
//! infix operators as long as their precedence is high enough. This elegantly
//! handles operator precedence and associativity without a grammar rule per
//! precedence level. The token -> handler mapping lives in `parse_rule`.
//!
//! ## What the compiler produces
//! A `Function` object (wrapped in `Obj::Fun`) whose `Chunk` holds the emitted
//! bytecode. The top-level script is itself compiled as an implicit function.

use crate::chunk::Chunk;
use crate::common::{FatPointer, Function, FunctionType, Obj, OpCode, Value};
use crate::hash_map::Table;
use crate::hasher;
use crate::memory;
use crate::scanner::{Scanner, Token, TokenType};
use num_derive::FromPrimitive;

extern crate num;

/// Operator binding powers, ordered lowest (`None`) to highest (`Primary`).
///
/// Pratt parsing works by comparing these values: an infix operator is only
/// consumed if its precedence is `>=` the precedence we are currently parsing
/// at. Because the enum is `#[repr(u8)]` with explicit ascending values, we
/// can compare and do arithmetic on them as plain integers (see the `+1`
/// trick in `binary`). `FromPrimitive` lets us turn a `u8` back into a
/// `Precedence` after that arithmetic.
///
/// Note the values start at 1, not 0 — so `None` is 1. This is deliberate so
/// that `Precedence::None` is still a valid `FromPrimitive` result and the
/// `+1` step in `binary` never overflows past the top of the range.
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

// The parse table below maps each token type to its prefix/infix handlers.
// Each handler is a function pointer (`ParseFn`) stored behind an `Option`,
// where `None` (NOOP) means "this token has no handler in that position".
// Wrapping each method in a closure lets us store a uniform function-pointer
// type even though the real work lives in `Compiler`'s methods.
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

/// The heart of the Pratt parser: given a token type, return its parse rule
/// (prefix handler, infix handler, and the precedence to use when this token
/// acts as an infix operator).
///
/// This is a pure function of the token type — it needs no compiler state —
/// which is why it's a free function rather than a method. Examples of the
/// encoding:
///   - `(` has a prefix rule (`grouping`, for `(expr)`) AND an infix rule
///     (`call`, for `callee(args)`), with `Call` precedence.
///   - `-` has both a prefix rule (unary negation) and an infix rule
///     (subtraction).
///   - `+` has only an infix rule; there is no prefix `+` in Lox.
///   - a number/string/identifier has only a prefix rule.
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

/// A parse handler: a plain function pointer taking the compiler and a
/// `can_assign` flag. Using `fn(...)` (not a closure type) keeps every handler
/// the same concrete type so they can live together in the `ParseRule` table.
/// `can_assign` tells a handler whether an `=` assignment is allowed to follow
/// it in this position (see `parse_precedence` / `variable`).
type ParseFn = fn(compiler: &mut Compiler, can_assign: bool);

/// One row of the Pratt parse table for a given token type.
struct ParseRule {
    /// Handler used when the token starts an expression (e.g. `-x`, `(x)`).
    prefix: Option<ParseFn>,
    /// Handler used when the token sits between operands (e.g. `a + b`).
    infix: Option<ParseFn>,
    /// Binding power of this token as an infix operator.
    precedence: Precedence,
}

/// Tracks the parser's position and error state as we stream tokens.
///
/// The parser only ever holds two tokens at once — a one-token lookahead.
/// That is all a single-pass compiler needs.
#[derive(Debug, Clone)]
struct Parser {
    /// The next token to be consumed (lookahead).
    current: Option<Token>,
    /// The most recently consumed token (what we're acting on right now).
    previous: Option<Token>,
    /// Set once any parse error has occurred; the final result is rejected.
    had_error: bool,
    /// True while recovering from an error, to suppress cascading error
    /// messages until we resynchronize at a statement boundary.
    panic_mode: bool,
}

/// A local variable slot recorded at compile time.
///
/// The compiler mirrors, at compile time, the layout the VM will have on its
/// value stack at run time: local variable N lives at a fixed stack offset.
/// Recording `(name, scope_depth)` lets us resolve an identifier to that
/// offset and know which block it belongs to.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Local {
    /// An occupied slot: the declaring token (its name) and the scope depth
    /// at which it was declared.
    Filled(Token, usize),
    /// A reserved/unused slot. Slot 0 of every function is `Empty` on purpose
    /// (see `CompilerContext::init`).
    Empty,
}

/// An upvalue: a variable captured by a closure from an enclosing function.
///
/// When an inner function refers to a variable of an outer function, it can't
/// use a normal local slot (that lives in the outer function's stack frame).
/// Instead the closure captures it as an *upvalue*. This enum records, for
/// the closure being compiled, where to find each captured variable.
#[derive(Debug, Clone, Copy)]
pub(crate) enum UpValue {
    /// `Filled(index, is_local)`:
    ///   - `index`   — slot to capture from the *immediately enclosing*
    ///                 function.
    ///   - `is_local` — true  => capture a *local* of the enclosing function;
    ///                  false => capture an *upvalue* of the enclosing
    ///                           function (i.e. it was itself captured from
    ///                           further out — this is how capture chains
    ///                           through multiple nesting levels).
    Filled(u8, bool),
    Empty,
}

/// Per-function compilation state.
///
/// Each function being compiled (including the top-level script) gets its own
/// context: its own bytecode chunk, its own set of locals, and its own set of
/// upvalues. When we start compiling a nested function we push a new context;
/// when we finish we pop it. The stack of contexts lives in `Compiler`.
#[derive(Debug, Clone)]
pub(crate) struct CompilerContext {
    /// The function object being built. `Obj::Fun` holds the `Chunk` that
    /// receives all emitted bytecode for this function.
    function: Obj,
    /// Local variables in scope, ordered by declaration. Its `.len()` is the
    /// number of locals in use — there is deliberately no separate count
    /// field, because a `Vec`'s length already *is* that count. Keeping a
    /// second counter in sync by hand was the source of an earlier scoping
    /// bug, so `push`/`truncate`/`len` are the single source of truth.
    locals: Vec<Local>,
    /// Upvalues captured by this function, indexed 0..up_value_count.
    up_values: Vec<UpValue>,
    /// How many entries of `up_values` are actually filled.
    // NOTE: this is the same "Vec + separate count" pattern that caused the
    // locals bug; `up_values` is pre-sized to 255 so `.len()` can't stand in
    // for the count here, hence the explicit counter.
    up_value_count: usize,
}

impl CompilerContext {
    fn init() -> CompilerContext {
        let mut locals = vec![];
        // Reserve stack slot 0. At run time the VM places the function/closure
        // being called in slot 0 of its call frame (later used for `this` in
        // methods). By pushing one `Empty` placeholder, user locals start at
        // index 1 and their compile-time indices line up with the runtime
        // stack offsets. Without this, every local would be off by one.
        locals.push(Local::Empty);

        let mut up_values = vec![];
        up_values.resize(u8::MAX as usize, UpValue::Empty);

        CompilerContext {
            locals,
            up_values,
            up_value_count: 0,
            function: Obj::Fun(Function::new_function(FunctionType::Script)),
        }
    }

    /// Record how many parameters this function takes. Arity is checked
    /// against the argument count at call time by the VM.
    fn update_function_arity(&mut self, arity: u8) {
        if let Obj::Fun(function) = &mut self.function {
            function.arity = arity;
        }
    }
}

/// The compiler itself. Owns the scanner and parser, and drives compilation
/// of the whole program.
///
/// The `'c` lifetime ties the compiler to the interned-string `table` it
/// borrows from the VM — the compiler does not own that table, it just uses
/// it while compiling.
pub(crate) struct Compiler<'c> {
    /// Interned-string table, borrowed from the VM. Used for string interning
    /// so identical string literals/identifiers share one heap allocation.
    table: &'c mut Table<Value>,
    /// Produces tokens on demand from the source text.
    scanner: Scanner,
    /// One-token lookahead + error state.
    parser: Parser,
    /// The full source text; tokens carry byte offsets into this string.
    source: String,
    /// Index into `contexts` of the function currently being compiled.
    // NOTE: clox links compiler contexts with a parent pointer; here they're
    // stored in a Vec and referenced by this index. Nested functions push a
    // context and bump this index; finishing one decrements it.
    current_context: usize,
    /// Current lexical nesting depth. 0 = global (top-level) scope; each
    /// `{ ... }` block or function body increments it. Locals record the
    /// depth they were declared at so `end_scope` knows which to discard.
    scope_depth: usize,
    /// Stack of per-function contexts (see `CompilerContext`).
    contexts: Vec<CompilerContext>,
}

impl<'c> Compiler<'c> {
    /// Build a fresh compiler with a single top-level (script) context.
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

    /// Compile an entire program.
    ///
    /// Prime the scanner, take the first token, then repeatedly parse
    /// top-level declarations until EOF. Returns `(had_error, function)` where
    /// `function` is the compiled top-level script (as an `Obj::Fun`). The
    /// caller (VM) checks `had_error` before running.
    pub(crate) fn compile(&mut self, source: String) -> (bool, Obj) {
        self.source = source;
        let chars: Vec<char> = self.source.chars().collect();
        self.scanner.refresh(0, self.source.len(), chars);
        // Prime the pump: load `current` with the first token so the very
        // first `advance` shifts it into `previous` as expected.
        self.advance();
        while !self.match_token(TokenType::Eof) {
            self.declaration();
        }
        self.end_compiler();
        (
            self.parser.had_error,
            self.current_context_mut().function.clone(),
        )
    }

    /// Move the lookahead forward: `previous <- current`, then scan a new
    /// `current`. Error tokens from the scanner are reported and skipped, so
    /// the rest of the compiler never has to handle a `TokenType::Error`.
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

    /// A *declaration* is the top grammar rule for a statement position that
    /// may introduce a name: `fun`, `var`, or any plain `statement`.
    ///
    /// After each declaration, if we entered panic mode from an error, we
    /// resynchronize so one syntax error doesn't produce a flood of spurious
    /// follow-on errors.
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

    /// Compile a function declaration `fun name(params) { body }`.
    ///
    /// The name is declared like any variable, the function body is compiled
    /// by `function()` (which leaves a closure value on the stack), and then
    /// we bind that value to the name via `DefineGlobalVariable`.
    fn fun_decl(&mut self) {
        let index = self.parse_variable();
        let prev_token = self.parser.previous.unwrap();
        self.function();
        self.emit_opcode(OpCode::DefineGlobalVariable);
        self.current_chunk().write_index(index, prev_token.line);
    }

    /// Compile a function's parameter list and body into a *new* context, then
    /// emit a `Closure` instruction (plus its upvalue metadata) into the
    /// *enclosing* function's chunk.
    ///
    /// Flow:
    ///   1. Push a fresh `CompilerContext` and begin a new scope.
    ///   2. Parse parameters (each becomes a local of the new function).
    ///   3. Compile the `{ body }`.
    ///   4. Pop this context, take the finished inner function + its upvalues.
    ///   5. In the enclosing chunk, store the inner function as a constant and
    ///      emit `Closure` followed by, for each upvalue, a `(is_local, index)`
    ///      byte pair the VM will use to wire up captures.
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
        // Parse comma-separated parameters: one, then zero or more `, param`.
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

        // BUG: `arity` is a u8, so its maximum value is 255; `arity >= 255`
        // can only ever be true at exactly 255, and a 256th parameter would
        // overflow the u8 (panicking in debug) before this check runs. clippy
        // flags this as `absurd_extreme_comparisons`. clox checks the count
        // *before* incrementing (`if arity == 255 { error }`).
        if arity >= 255 {
            self.error_at_current("Can't have more than 255 parameters.");
        }

        self.current_context_mut().update_function_arity(arity);
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
        // `end_compiler` decrements `current_context` back to the enclosing
        // function, so the finished inner context is now at
        // `current_context + 1`.
        self.end_compiler();
        let inner_function = self.contexts[self.current_context + 1].function.clone();
        let up_values = self.contexts[self.current_context + 1].up_values.clone();
        self.contexts.remove(self.current_context + 1);
        // Store the compiled inner function as a constant in the *enclosing*
        // chunk, then emit the instruction that builds a closure from it.
        let constant_index = self
            .current_chunk()
            .add_constant(Value::from(inner_function));
        self.emit_opcode(OpCode::Closure);
        self.emit_byte(constant_index as u8);
        // Emit two bytes per upvalue: an is_local flag (1/0) and the index.
        // The VM reads these right after the Closure opcode to capture each
        // variable.
        // NOTE (incomplete feature): the VM side of closures is not finished
        // — its `Closure` handler does not currently consume these upvalue
        // bytes, and there are no runtime handlers for Get/SetUpValue. So
        // closures that actually capture variables do not work end-to-end
        // yet; this compiler half is ahead of the VM half.
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

    /// Resolve `name` as an *upvalue*: a variable that lives in some enclosing
    /// function, captured by the (possibly deeply nested) function we are
    /// compiling.
    ///
    /// Core idea (upvalue resolution): a closure can't reach an outer
    /// function's stack directly, so at each nesting level we record "capture
    /// slot X from the function one level out". If the variable is a *local*
    /// of the immediately-enclosing function, we capture it directly
    /// (`is_local = true`). Otherwise we recurse outward; each intermediate
    /// function also records an upvalue (`is_local = false`) that forwards the
    /// capture inward. This threads the variable down through every level.
    ///
    /// Returns the upvalue index to use, or -1 if `name` isn't found in any
    /// enclosing scope (meaning it must be a global).
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

    /// Append an upvalue entry to the given function context and return via
    /// its `up_value_count` bump. Called by `recursive_resolve_up_value` for
    /// each level that participates in a capture.
    fn add_up_value(&mut self, index: u8, is_local: bool, context_index: usize) {
        let up_value = UpValue::Filled(index, is_local);
        let up_value_count = self.contexts[context_index].up_value_count;
        self.contexts[context_index].up_values[up_value_count] = up_value;
        self.contexts[context_index].up_value_count += 1;
    }

    /// Parse one function parameter and immediately define it as a local of
    /// the function being compiled.
    fn parse_and_define_parameter(&mut self) {
        let param_index = self.parse_variable();
        self.define_variable(param_index);
    }

    /// Compile a `var name = expr;` (or `var name;`) declaration.
    ///
    /// If there's no initializer, we emit `Nil` so the variable still has a
    /// value on the stack. `define_variable` then binds it (globally) or, for
    /// a local, simply leaves the value in its stack slot.
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

    /// Consume the variable name token, declare it (for locals), and return
    /// the constant-pool index of its name string.
    ///
    /// For a *global* (`scope_depth == 0`) we intern the name as a string
    /// constant and return its index, because globals are looked up by name
    /// at run time. For a *local* the name isn't needed at run time (locals
    /// are addressed by stack slot), so the returned index is just 0 and
    /// unused.
    fn parse_variable(&mut self) -> usize {
        self.consume(TokenType::Identifier, "Expected name after variable");
        self.declare_variable();
        let mut index = 0;
        // NOTE: `scope_depth` is usize, so `<= 0` is equivalent to `== 0`
        // (clippy flags this). It means "we're at global scope".
        if self.scope_depth <= 0 {
            index = self.identifier();
        }
        index
    }

    /// Record a *local* variable at the current scope depth.
    ///
    /// Does nothing at global scope (globals are handled by name, not slot).
    /// For locals it appends to `locals` (whose length is the live count),
    /// after checking for the 255-slot limit and for an illegal redeclaration
    /// of the same name in the same scope.
    fn declare_variable(&mut self) {
        if self.scope_depth > 0 {
            if self.current_context_mut().locals.len() == 255 {
                self.error("Too many local variables in function.");
                return;
            }
            let token = self.parser.previous.unwrap();
            let local = Local::Filled(token, self.scope_depth);

            let matching_token = self.resolve_local(token);

            if matching_token != -1 {
                self.error("Already a variable with this name in this scope.");
            }
            self.current_context_mut().locals.push(local);
        }
    }

    /// Resolve `token` to a local-variable stack slot in the current function,
    /// or -1 if it isn't a local (so the caller then tries upvalue/global).
    // NOTE: `.locals.clone()` copies the slot list so it can be passed to
    // `resolve_from_locals` without holding a borrow of `self` across that
    // call. It's a borrow-checker workaround, not a data requirement.
    fn resolve_local(&mut self, token: Token) -> i32 {
        if self.current_context_mut().locals.len() <= 0 {
            return -1;
        }
        let scope_depth = self.scope_depth;
        let locals = self.current_context_mut().locals.clone();

        if let Some(value) = self.resolve_from_locals(locals, scope_depth, token) {
            return value;
        }
        -1
    }

    /// Search a slot list for a local matching `token`'s name, scanning from
    /// the top of the stack downward (`.rev()`) so the *innermost* declaration
    /// wins — that's how variable shadowing resolves to the nearest binding.
    ///
    /// The stack slot index (position in `locals`) is exactly the run-time
    /// stack offset the VM uses, which is why the compile-time and run-time
    /// layouts must agree (see the reserved slot 0).
    fn resolve_from_locals(
        &self,
        locals: Vec<Local>,
        scope_depth: usize,
        token: Token,
    ) -> Option<i32> {
        for (idx, existing) in locals.iter().enumerate().rev() {
            match existing {
                Local::Filled(existing_token, depth) => {
                    if depth.ge(&scope_depth) {
                        // Fast reject on length before comparing name bytes.
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

    /// Prefix handler for an identifier used in an expression.
    ///
    /// Resolves the name in three tiers, mirroring lexical scoping rules:
    ///   1. a *local* of the current function (`Get/SetLocalVariable`),
    ///   2. else an *upvalue* captured from an enclosing function
    ///      (`Get/SetUpValue`),
    ///   3. else a *global* looked up by name (`Get/SetGlobalVariable`).
    ///
    /// Whether it emits a get or a set depends on `can_assign` and whether an
    /// `=` follows: `x = expr` compiles the value then a set; a bare `x`
    /// compiles a get.
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

    /// Bind a just-declared variable to its initializer value.
    ///
    /// For a *local*, there is nothing to emit: the initializer's value is
    /// already sitting in the correct stack slot, so we just return. For a
    /// *global*, emit `DefineGlobalVariable` with the name-constant index so
    /// the VM records the binding in its globals table.
    fn define_variable(&mut self, index: usize) {
        if self.scope_depth > 0 {
            return;
        }
        let prev_token = self.previous_token();
        self.emit_opcode(OpCode::DefineGlobalVariable);
        self.current_chunk().write_index(index, prev_token.line);
    }

    /// Intern the previous token's text as a string constant and return its
    /// index. Reused for variable *names* (globals are keyed by name string).
    fn identifier(&mut self) -> usize {
        self.string(false, false)
    }

    /// Dispatch a single statement to its handler based on the current token.
    /// A bare expression followed by `;` is an *expression statement*.
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

    /// Compile `return;` or `return expr;`. A bare return emits an implicit
    /// `nil` return value (via `emit_return`); otherwise the expression's
    /// value is returned.
    fn return_stmt(&mut self) {
        if self.match_token(TokenType::Semicolon) {
            self.emit_return();
        } else {
            self.expression();
            self.consume_semicolon();
            self.emit_opcode(OpCode::Return);
        }
    }

    /// Compile a C-style `for (init; cond; incr) body`.
    ///
    /// All three clauses are optional. The tricky part is that `incr` textually
    /// comes before `body` but must *run after* it each iteration. In a
    /// single-pass compiler with no AST we can't reorder code, so we use jumps:
    /// after the condition we jump *over* the increment to the body, run the
    /// body, then loop *back* to the increment, which then loops back to the
    /// condition. `loop_start` is repointed to the increment so the body's
    /// back-edge lands there. The whole thing is wrapped in a scope so a
    /// `var` in the init clause is local to the loop.
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

    /// Compile `while (cond) body`.
    ///
    /// `loop_start` records the bytecode offset of the condition *before* we
    /// emit it, so that after the body we can emit a backward `Loop` jump to
    /// re-test the condition. `JumpIfFalse` skips the body (and its back-edge)
    /// when the condition is false. The two `Pop`s discard the condition value
    /// the comparison leaves on the stack, on both the true and false paths.
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

    /// Emit a `Loop` (backward jump) back to `loop_start`.
    ///
    /// The operand is the *distance to jump back*, computed now because we
    /// already know `loop_start` (it's behind us). `+2` accounts for the two
    /// operand bytes we're about to emit, which the VM will have advanced past
    /// before applying the jump. The two `emit_byte`s write the 16-bit
    /// distance big-endian (high byte first).
    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_opcode(OpCode::Loop);
        let jump = (self.current_chunk().code.len() - loop_start + 2) as u16;

        // BUG: `jump` is already a u16, so `jump > u16::MAX` is always false
        // (clippy: absurd_extreme_comparisons). An over-long loop would wrap
        // when cast to u16 rather than being reported. The check should be
        // done on the usize distance before the cast.
        if jump > u16::MAX {
            self.error(format!("Can not jump more than {:?} bytes", u16::MAX).as_str());
        } else {
            self.emit_byte(((jump >> 8) & 0xff) as u8);
            self.emit_byte((jump & 0xff) as u8);
        }
    }

    /// Compile `if (cond) thenBranch [else elseBranch]`.
    ///
    /// Two forward jumps are used: `JumpIfFalse` skips the then-branch when the
    /// condition is false, and an unconditional `Jump` makes the then-branch
    /// skip over the else-branch. Their targets aren't known when emitted, so
    /// they're *backpatched* by `patch_jump` once we've compiled each branch.
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

    /// Emit a jump instruction with a *placeholder* 16-bit operand and return
    /// the offset of that placeholder so it can be patched later.
    ///
    /// This is the *backpatching* technique: in a single pass we don't yet
    /// know how far the jump must reach (we haven't compiled the code it skips
    /// over), so we write `0xffff` now and come back to overwrite it in
    /// `patch_jump` once the target is known.
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

    /// Fill in a jump placeholder emitted earlier by `emit_jump`.
    ///
    /// `offset` points at the two placeholder bytes. The distance to jump is
    /// "how much code we've emitted since the placeholder", minus 2 for the
    /// operand bytes themselves (which the VM will have already stepped past).
    /// The 16-bit distance is written big-endian into the two bytes.
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

        // BUG: same as `emit_loop` — `jump` is a u16 so `jump > u16::MAX` can
        // never be true (clippy: absurd_extreme_comparisons). A too-large jump
        // silently wraps instead of erroring.
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

    /// Compile the declarations inside a `{ ... }` up to the closing brace.
    /// The caller is responsible for `begin_scope`/`end_scope` around it.
    fn block(&mut self) {
        while !self.check(TokenType::RightBrace) && !self.check(TokenType::Eof) {
            self.declaration();
        }

        self.consume(TokenType::RightBrace, "Expect '}' after block.");
    }

    /// Enter a new lexical scope (just bump the depth counter).
    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    /// Leave the current lexical scope and discard the locals it introduced.
    ///
    /// Two things must happen when a scope ends: (1) at run time the local
    /// values sitting on the VM stack must be popped, so we emit one `Pop` per
    /// discarded local; (2) at compile time those slots must be forgotten so
    /// they no longer resolve. We `truncate` the `locals` vec to drop exactly
    /// the trailing entries that belong to the exited scope.
    ///
    /// Using `truncate` (rather than only decrementing a counter) is what
    /// fixed an earlier bug: previously the vec kept stale `Filled` slots that
    /// still resolved after their scope had ended, so an inner loop variable
    /// could shadow an outer/global variable of the same name.
    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        // Copy scope_depth into a local so the closure below borrows this
        // `usize` instead of `self`; otherwise it would conflict with the
        // `&self` borrow taken by `current_context()` during iteration.
        let scope_depth = self.scope_depth;
        // Count trailing locals that belonged to the scope we just left
        // (those declared at a deeper depth than the new current depth).
        let scoped_locals = self
            .current_context()
            .locals
            .iter()
            .filter(|local| match local {
                Local::Filled(_, depth) => depth.gt(&scope_depth),
                _ => false,
            })
            .count();
        let local_len = self.current_context_mut().locals.len();
        if local_len == 0 {
            return;
        }
        for _ in 1..=scoped_locals {
            self.emit_opcode(OpCode::Pop);
        }
        // Drop the discarded slots from the end; the new length is the live
        // local count. `truncate` takes the length to KEEP, not to remove.
        self.current_context_mut().locals.truncate(local_len - scoped_locals);
    }

    /// An expression used as a statement (e.g. a function call). The
    /// expression leaves a value on the stack that nothing consumes, so we
    /// emit a `Pop` to discard it and keep the stack balanced.
    fn expression_statement(&mut self) {
        self.expression();
        self.consume_semicolon();
        self.emit_opcode(OpCode::Pop);
    }

    /// Require a `;`. Convenience wrapper over `consume`.
    fn consume_semicolon(&mut self) {
        self.consume(
            TokenType::Semicolon,
            "Expected semicolon at the end of experession",
        );
    }

    /// Error recovery. After a syntax error we're in `panic_mode`; skip tokens
    /// until we reach a likely statement boundary (a `;`, or the start of a
    /// new declaration/statement keyword) so parsing can resume without a
    /// cascade of bogus errors from the same mistake.
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

    /// Compile `print expr;` — evaluate the expression, then emit `Print`
    /// which pops and prints the top of the stack. `print` is a statement in
    /// Lox, not a function.
    fn print_stmt(&mut self) {
        self.expression();
        self.consume_semicolon();
        self.emit_opcode(OpCode::Print);
    }

    /// If the current token is `token_type`, consume it and return true;
    /// otherwise leave it and return false. This is the standard "optional
    /// token" primitive used throughout the parser.
    fn match_token(&mut self, token_type: TokenType) -> bool {
        if self.check(token_type) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Non-consuming lookahead: is the current token of this type?
    fn check(&self, token_type: TokenType) -> bool {
        match self.parser.current {
            Some(current_token) => current_token.token_type == token_type,
            None => false,
        }
    }

    /// Report an error located at the *current* (lookahead) token.
    fn error_at_current(&mut self, message: &str) {
        self.error_at(message, self.parser.current.unwrap())
    }

    /// Report an error located at the *previous* (just-consumed) token.
    fn error(&mut self, message: &str) {
        self.error_at(message, self.parser.previous.unwrap())
    }

    /// Print an error message tied to a token and mark the parse as failed.
    ///
    /// If already in `panic_mode` we suppress the message (we're mid-recovery)
    /// but do nothing else. Otherwise we enter panic mode, print the location,
    /// and set `had_error` so `compile` ultimately returns failure.
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

    /// Require the current token to be `token_type` and advance past it;
    /// otherwise report `message` at the current token. This is how the parser
    /// enforces required syntax (`)`, `;`, `{`, etc.).
    fn consume(&mut self, token_type: TokenType, message: &str) {
        if self.parser.current.unwrap().token_type == token_type {
            self.advance();
            return;
        }

        self.error_at_current(message);
    }

    /// Append a raw byte to the current function's chunk, tagging it with the
    /// current source line (for run-time error reporting). This is the lowest-
    /// level emit primitive; everything else builds on it.
    fn emit_byte(&mut self, byte: u8) {
        let prev_token = self.previous_token();
        self.current_chunk().write_chunk(byte, prev_token.line);
    }

    /// Emit two raw bytes in sequence.
    fn emit_bytes(&mut self, byte_1: u8, byte_2: u8) {
        self.emit_byte(byte_1);
        self.emit_byte(byte_2);
    }

    /// Emit one opcode (opcodes are just `u8` values under the hood).
    fn emit_opcode(&mut self, opcode: OpCode) {
        self.emit_byte(opcode as u8);
    }

    /// Emit two opcodes in sequence. Used where one Lox operator maps to two
    /// VM instructions, e.g. `>=` becomes `Less` then `Not` (see
    /// `emit_operator`).
    fn emit_opcodes(&mut self, opcode_1: OpCode, opcode_2: OpCode) {
        self.emit_opcode(opcode_1);
        self.emit_opcode(opcode_2);
    }

    /// Finish compiling the current function: emit an implicit return, then
    /// pop back to the enclosing context. `current_context` is only
    /// decremented for inner functions (index 0 is the top-level script and
    /// has no enclosing context).
    fn end_compiler(&mut self) {
        self.emit_return();
        // do it only for inner functions
        if self.current_context > 0 {
            self.current_context -= 1;
        }
    }

    /// Emit an implicit `return nil;`. Every function ends with this so that a
    /// function with no explicit `return` still returns a value and the VM
    /// always has something to pop.
    fn emit_return(&mut self) {
        self.emit_opcode(OpCode::Nil);
        self.emit_opcode(OpCode::Return);
    }

    /// Parse a full expression. Expressions start at `Assignment` precedence,
    /// the lowest that still allows an assignment target.
    fn expression(&mut self) {
        self.parse_precedence(Precedence::Assignment);
    }

    /// Add `value` to the current chunk's constant pool and emit the
    /// instruction to push it at run time. Returns the constant's index.
    fn emit_constant(&mut self, value: Value) -> usize {
        let prev_token = self.previous_token();
        self.current_chunk().write_constant(value, prev_token.line)
    }

    /// Parse the previous token's text as an `f64`. Takes `&self` because it
    /// only reads (it doesn't emit) — an example of not demanding `&mut` when
    /// a shared borrow suffices.
    fn str_to_float(&self, token: Token) -> f64 {
        let value = self.token_name(token);
        value.parse::<f64>().unwrap()
    }

    /// Prefix handler for a number literal: parse it and emit it as a constant.
    fn number(&mut self, _can_assign: bool) {
        let value: f64 = self.str_to_float(self.parser.previous.unwrap());
        self.emit_constant(Value::from(value));
    }

    /// Infix handler for `and` (logical short-circuit).
    ///
    /// If the left operand is falsey we must skip the right operand entirely
    /// and leave the (falsey) left value as the result. `JumpIfFalse` (which
    /// does not pop) jumps past the right operand; otherwise we `Pop` the left
    /// value and evaluate the right, whose value becomes the result.
    fn and(&mut self, _can_assign: bool) {
        let offset = self.emit_jump(OpCode::JumpIfFalse);
        self.emit_opcode(OpCode::Pop);
        self.parse_precedence(Precedence::And);
        self.patch_jump(offset);
    }

    /// Infix handler for `or` (logical short-circuit).
    ///
    /// Mirror of `and`: if the left operand is truthy we skip the right and
    /// keep the left value. Implemented with a `JumpIfFalse` over an
    /// unconditional `Jump`: falsey falls through to evaluate the right,
    /// truthy takes the `Jump` to the end keeping the left value.
    fn or(&mut self, _can_assign: bool) {
        let else_jump_offset = self.emit_jump(OpCode::JumpIfFalse);
        let end_jump_offset = self.emit_jump(OpCode::Jump);
        self.patch_jump(else_jump_offset);
        self.emit_opcode(OpCode::Pop);
        self.parse_precedence(Precedence::Or);
        self.patch_jump(end_jump_offset);
    }

    /// Infix handler for a call `callee(arg, arg, ...)`.
    ///
    /// The callee is already on the stack (parsed as the prefix). We compile
    /// each argument expression (leaving its value on the stack), then emit
    /// `Call` with the argument count so the VM knows how many stack slots the
    /// call consumes.
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

    /// Turn the previous token's text into a string `Value`, with *interning*.
    ///
    /// Interning: rather than allocate a fresh copy for every occurrence of
    /// the same string, we look it up in the shared `table`. If it's already
    /// there we reuse the existing allocation; otherwise we allocate once and
    /// insert it. This makes identical strings share storage and lets string
    /// equality be a fast pointer comparison. Used both as the prefix handler
    /// for string literals and to intern variable *names* (`emit_constant`
    /// distinguishes: emit a load instruction vs. just add to the pool).
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

    /// Build a string `Value` from an already-interned pointer and add it to
    /// the constant pool (emitting a load if `emit_constant`).
    fn reuse_existing_string(&mut self, existing: FatPointer, emit_constant: bool) -> usize {
        let obj_string = Obj::from(existing);
        let value = Value::from(obj_string);
        if emit_constant {
            self.emit_constant(value)
        } else {
            self.current_chunk().add_constant(value)
        }
    }

    /// Allocate a brand-new interned string: copy its bytes into manually
    /// managed memory, wrap it in a `FatPointer`, insert it into the intern
    /// table, and add it to the constant pool.
    ///
    /// Storage is sized to `str_value.len()` via `memory::allocate_bytes`, i.e.
    /// the exact byte length of the text. (This used to call
    /// `allocate::<String>()`, which reserved the 24-byte `String` struct header
    /// instead of the text length and overflowed on longer strings.)
    fn create_new_string(
        &mut self,
        mut str_value: String,
        hash_value: u32,
        emit_constant: bool,
    ) -> usize {
        let str_ptr = memory::allocate_bytes(str_value.len());
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

    /// Look up an already-interned string by content+hash, if present.
    fn get_existing_string(&self, str_value: &str, hash_value: u32) -> Option<&FatPointer> {
        let exiting_value = self.table.find_entry_with_value(str_value, hash_value);
        exiting_value
    }

    /// Extract the previous token's text (owned) and its hash, for interning.
    fn prev_token_to_string(& self) -> (String, u32) {
        let token = self.parser.previous.unwrap();

        let str_value = if token.token_type == TokenType::String {
            self.source[token.start + 1.. token.start + token.length - 1].to_owned()
        } else {
            self.token_name(token).to_owned()
        };

        let hash_value = hasher::hash(&str_value);
        (str_value, hash_value)
    }

    /// Prefix handler for `(`: parse the inner expression and require the
    /// closing `)`. Grouping only affects parsing precedence; it emits no
    /// instruction of its own.
    fn grouping(&mut self, _can_assign: bool) {
        self.expression();
        self.consume(TokenType::RightParen, "Expect ')' after expression");
    }

    /// Prefix handler for a unary operator (`-x`, `!x`).
    ///
    /// Note the ordering: we compile the *operand first* (so its value is on
    /// the stack), then emit the operator, which pops that value and pushes
    /// the result. Operand is parsed at `Unary` precedence so unary binds
    /// tighter than binary operators.
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

    /// Map a binary operator token to the opcode(s) that implement it.
    ///
    /// Some operators are implemented as a pair to avoid dedicated opcodes:
    /// `>=` is `Less` then `Not` (i.e. `!(a < b)`), `<=` is `Greater` + `Not`,
    /// and `!=` is `Equal` + `Not`. Fewer opcodes, same semantics.
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

    /// Infix handler for a binary operator (`+`, `*`, `<`, `==`, ...).
    ///
    /// When we get here the left operand is already compiled. We parse the
    /// right operand at *one higher* precedence than this operator, then emit
    /// the operator so it pops both operands. The `+1` is the classic trick
    /// that makes binary operators *left-associative*: `a - b - c` parses as
    /// `(a - b) - c`, because after `a - b` the loop won't re-consume another
    /// `-` at the same precedence. (A right-associative operator would recurse
    /// at the *same* precedence instead.)
    fn binary(&mut self, _can_assign: bool) {
        let operator_type = self.parser.previous.unwrap().token_type;
        let rule = parse_rule(operator_type);
        let next_op: Precedence = num::FromPrimitive::from_u8((rule.precedence) as u8 + 1).unwrap();
        self.parse_precedence(next_op);
        self.emit_operator(operator_type);
    }

    /// Prefix handler for the keyword literals `true`, `false`, `nil`.
    fn literal(&mut self, _can_assign: bool) {
        let token_type = self.parser.previous.unwrap().token_type;
        match token_type {
            TokenType::False => self.emit_opcode(OpCode::False),
            TokenType::Nil => self.emit_opcode(OpCode::Nil),
            TokenType::True => self.emit_opcode(OpCode::True),
            _ => println!("Unknown type: {:?} ", token_type),
        }
    }

    /// Slice the source text for a token using its byte offset and length.
    /// Tokens store positions, not copies, so this is how we get their text.
    fn token_name(&self, token: Token) -> &str {
        &self.source[token.start..token.start + token.length]
    }

    /// The engine of the Pratt parser — parse any expression whose operators
    /// bind at least as tightly as `precedence`.
    ///
    /// Steps:
    ///   1. Advance and run the *prefix* handler for the token that starts the
    ///      expression. If there's no prefix handler, that token can't begin
    ///      an expression -> syntax error.
    ///   2. While the *next* token is an infix operator whose precedence is
    ///      `>=` `precedence`, consume it and run its *infix* handler. This
    ///      loop is what climbs the precedence ladder.
    ///   3. `can_assign` is true only at low (assignment-level) precedence, so
    ///      that `a = 1` is allowed but `a + b = 1` is not; a leftover `=`
    ///      here means an invalid assignment target.
    fn parse_precedence(&mut self, precedence: Precedence) {
        self.advance();
        let prefix = parse_rule(self.parser.previous.unwrap().token_type).prefix;

        if prefix.is_none() {
            self.error("Expect expression");
            return;
        }

        // Only allow `=` to follow when we're parsing at assignment precedence
        // or lower; this prevents treating e.g. `a + b` as an assignment
        // target. Passed down into the prefix/infix handlers.
        let can_assign = precedence as u8 <= Precedence::Assignment as u8;

        let prefix_func = prefix.unwrap();
        prefix_func(self, can_assign);

        while precedence as u8
            <= parse_rule(self.parser.current.unwrap().token_type).precedence as u8
        {
            self.advance();
            let infix = parse_rule(self.parser.previous.unwrap().token_type).infix;
            let infix_func = infix.unwrap();
            infix_func(self, can_assign);
        }
        if can_assign && self.match_token(TokenType::Equal) {
            self.error("Invalid assignment target");
        }
    }

    /// Mutable access to the bytecode chunk of the function being compiled.
    /// Almost all `emit_*` helpers route through here.
    fn current_chunk(&mut self) -> &mut Chunk {
        self.current_context_mut().function.get_func_chunk()
    }

    /// Mutable access to the current function's compile context.
    ///
    /// Paired with the read-only `current_context` below — the `get`/`get_mut`
    /// convention. Use this one only when actually mutating, so read-only call
    /// sites can take a shared borrow and avoid borrow-checker conflicts.
    fn current_context_mut(&mut self) -> &mut CompilerContext {
        self.contexts.get_mut(self.current_context).unwrap()
    }
    /// Shared (read-only) access to the current function's compile context.
    fn current_context(&self) -> &CompilerContext {
        self.contexts.get(self.current_context).unwrap()
    }
    /// The most recently consumed token (convenience accessor).
    fn previous_token(&self) -> Token {
        self.parser.previous.unwrap()
    }
}
