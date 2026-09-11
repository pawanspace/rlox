//! # Common types: the VM's "instruction set" and "value system"
//!
//! This file defines the shared vocabulary that the compiler and the VM both
//! speak:
//!   - `OpCode`  — the set of bytecode instructions the VM knows how to run.
//!   - `Value`   — the runtime representation of any Lox value (numbers,
//!                 booleans, nil, and heap objects).
//!   - `Obj`     — heap-allocated objects (strings, functions, closures).
//!   - `FatPointer` — this project's hand-rolled string representation.
//!
//! ## What is bytecode?
//! Instead of interpreting the source text (or an AST) directly, we compile the
//! program into a flat array of bytes called *bytecode*. Each instruction is
//! one byte — an *opcode* — optionally followed by operand bytes. The VM is a
//! loop that reads one opcode at a time and does what it says. This is the
//! design of clox (the C interpreter in "Crafting Interpreters"). Bytecode is
//! compact and cache-friendly, and the decode loop is a simple `match`.

use colored::Color;
use num_derive::FromPrimitive;
use rand::prelude::*;
use std::fmt::Debug;

use crate::{chunk::Chunk, hasher, memory};
use crate::vm::RuntimeError;

/// The VM's instruction set. Every compiled instruction begins with one of
/// these bytes.
///
/// ## Why `#[repr(u8)]` + explicit values + `FromPrimitive`?
/// - `#[repr(u8)]` forces the enum to be stored as a single byte, so we can
///   cast `opcode as u8` when *emitting* bytecode and store it directly in the
///   `Chunk`'s `code: Vec<u8>`.
/// - The explicit discriminants (`= 1`, `= 2`, ...) pin each opcode to a stable
///   number. If they drifted, previously compiled bytecode would mean something
///   different.
/// - `FromPrimitive` (from the `num` crates) generates the reverse mapping:
///   given a raw `u8` read from the code array, produce `Option<OpCode>`. That
///   is exactly what the VM's decode step needs. The `Option` is `None` for an
///   unknown byte, which is how the VM detects a corrupt/invalid instruction.
///
/// Values start at 1 (not 0) so that a zero byte is never a valid opcode.
#[derive(Debug)]
#[repr(u8)]
#[derive(FromPrimitive)]
pub(crate) enum OpCode {
    /// Return from the current function; the value on top of the stack is the
    /// result handed back to the caller.
    Return = 1,
    /// Push a constant onto the stack. Followed by a 1-byte index into the
    /// chunk's constant pool.
    Constant = 2,
    /// Like `Constant`, but followed by an 8-byte (usize) index, used when the
    /// constant pool has grown past what a single byte can address. See
    /// `Chunk::write_constant` for how the compiler chooses between the two.
    ConstantLong = 3,
    /// Negate the number on top of the stack (unary `-`).
    Negate = 4,
    /// Pop two values, push their sum (or concatenation, for strings).
    Add = 5,
    /// Pop two values, push `left - right`.
    Subtract = 6,
    /// Pop two values, push `left * right`.
    Multiply = 7,
    /// Pop two values, push `left / right`.
    Divide = 8,
    /// Push the `nil` value.
    Nil = 9,
    /// Push boolean `true`.
    True = 10,
    /// Push boolean `false`.
    False = 11,
    /// Logical not: pop one value, push its "falsiness" as a boolean.
    Not = 12,
    /// Pop two values, push whether they are equal.
    Equal = 13,
    /// Pop two values, push `left > right`.
    Greater = 14,
    /// Pop two values, push `left < right`.
    Less = 15,
    /// Pop one value and print it (the `print` statement).
    Print = 16,
    /// Discard the value on top of the stack. Used to drop expression results
    /// and to clean up locals when a scope ends.
    Pop = 17,
    /// Define a new global variable. Operand: constant index of the name.
    DefineGlobalVariable = 18,
    /// Assign to an existing global. Operand: constant index of the name.
    SetGlobalVariable = 19,
    /// Read a global's value onto the stack. Operand: constant index of name.
    GetGlobalVariable = 20,
    /// Assign to a local variable. Operand: the local's stack slot index.
    SetLocalVariable = 21,
    /// Read a local variable onto the stack. Operand: the local's slot index.
    GetLocalVariable = 22,
    /// Conditional jump: if the top of the stack is falsey, jump forward.
    /// Operand: a 2-byte forward offset. Used for `if`, `while`, `and`, `or`.
    JumpIfFalse = 23,
    /// Unconditional forward jump. Operand: a 2-byte offset.
    Jump = 24,
    /// Unconditional *backward* jump, used to loop. Operand: a 2-byte offset
    /// that is *subtracted* from the instruction pointer.
    Loop = 25,
    /// Call a function. Operand: the argument count (1 byte).
    Call = 26,
    /// Wrap a function into a closure and push it. Operand: constant index of
    /// the function, followed by upvalue-capture bytes.
    // NOTE: The closure/upvalue runtime is incomplete — the VM does not yet
    // consume the capture bytes, and SetUpValue/GetUpValue have no handlers.
    Closure = 27,
    /// Assign to a captured upvalue. Operand: the upvalue index.
    SetUpValue = 28,
    /// Read a captured upvalue onto the stack. Operand: the upvalue index.
    GetUpValue = 29,
}

/// A Lox runtime value — a *tagged union*.
///
/// A dynamically typed language must be able to store "any value" in one slot
/// (a stack entry, a variable). Rust's `enum` is exactly a tagged union: it
/// stores a hidden tag saying which variant is active, plus that variant's
/// payload. Pattern-matching on the variant is how the VM recovers the type at
/// runtime. This mirrors clox's `Value` struct (a tag + a C `union`), but
/// Rust's enum makes the tag/payload pairing memory-safe by construction.
///
/// Variants:
/// - `Boolean` — `true`/`false`.
/// - `Number`  — all Lox numbers are 64-bit floats (`f64`), like clox.
/// - `Obj`     — a heap-allocated object (string, function, closure).
/// - `Missing` — Lox's `nil` (the absence of a value).
#[derive(Debug, Clone)]
pub(crate) enum Value {
    Boolean(bool),
    Number(f64),
    Obj(Obj),
    Missing,
}

impl Value {
    /// True if this value is a boolean. (`#[inline]` asks the compiler to paste
    /// the body at the call site; these are tiny checks the hot VM loop calls
    /// often.)
    #[inline]
    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Boolean(_))
    }

    /// True if this value is `nil`.
    #[inline]
    pub fn is_missing(&self) -> bool {
        matches!(self, Value::Missing)
    }

    /// True if this value is a number.
    #[inline]
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    /// True if this value is any heap object.
    #[inline]
    pub fn is_obj(&self) -> bool {
        matches!(self, Value::Obj(_))
    }

    /// True if this value is specifically a heap *string* object.
    #[inline]
    pub fn is_obj_string(&self) -> bool {
        // NOTE: the `unsafe` block here is spurious — `Obj::is_string` is a
        // safe method (a plain `matches!`). The keyword has no effect and could
        // be removed.
        return match self {
            Value::Obj(obj) => unsafe { obj.is_string() },
            _ => false,
        };
    }
}

/// Value equality (`==`), used by `OpCode::Equal`.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        // BUG: `matches!(self, _other)` binds every value to a fresh catch-all
        // pattern named `_other`, so it is ALWAYS true. This guard is dead code;
        // the real logic is the inner `match`. (This pattern repeats in the
        // `Obj` and `FatPointer` impls below.)
        if matches!(self, _other) {
            return match (self, other) {
                (Value::Boolean(l), Value::Boolean(r)) => l == r,
                (Value::Number(l), Value::Number(r)) => l == r,
                (Value::Missing, Value::Missing) => true,
                // NOTE: `Obj` equality is pointer-identity (see `FatPointer`),
                // so two strings with equal *contents* built at runtime compare
                // as NOT equal.
                (Value::Obj(l), Value::Obj(r)) => l == r,
                _ => false,
            };
        }
        false
    }
}

// The `From<T> for Value` impls below are the idiomatic Rust way to define a
// conversion: implement `From`, and you get `Into` for free (a blanket impl in
// the standard library). We use these to wrap raw Rust values into `Value`s
// before pushing them on the VM stack.
impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Boolean(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Number(value)
    }
}

impl From<Obj> for Value {
    fn from(value: Obj) -> Self {
        Value::Obj(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConversionError {pub message: String}

// The `TryFrom<&Value>` impls below go the other way: unwrap a `Value` back
// into a raw Rust value. These conversions can *fail* (a `Value` might not hold
// the type asked for), so they use `TryFrom` (returning `Result`) rather than
// `Into` — the caller must handle the mismatch instead of getting a bogus
// default (`false`, `0.0`) or a panic. Implementing `TryFrom` also yields
// `TryInto` for free, so call sites can use `v.try_into()` / `X::try_from(v)`.
impl TryFrom<&Value> for bool {
    type Error = ConversionError;

    fn try_from(value: &Value) -> Result<bool, Self::Error> {
        match value {
            Value::Boolean(bool_value) => Ok(*bool_value),
            _ => Err(ConversionError{message: "cannot convert value to bool".to_string()}),
        }
    }
}
impl TryFrom<&Value> for f64 {
    type Error = ConversionError;

    fn try_from(value: &Value) -> Result<f64, Self::Error> {
        match value {
            Value::Number(f64_value) => Ok(*f64_value),
            _ => Err(ConversionError{message: "cannot convert value to f64".to_string()}),
        }
    }
}


impl TryFrom<&Value> for Obj {
    type Error = ConversionError;

    fn try_from(value: &Value) -> Result<Obj, Self::Error> {
        match value {
            Value::Obj(obj_value) => Ok(obj_value.clone()),
            _ => Err(ConversionError{message: "cannot convert value to obj".to_string()}),
        }
    }
}



impl TryFrom<&Value> for FatPointer {
    type Error = ConversionError;

    fn try_from(value: &Value) -> Result<FatPointer, Self::Error> {
        match value {
            Value::Obj(obj) => Ok(FatPointer::try_from(obj.clone()).unwrap()),
            _ => Err(ConversionError{message: "cannot convert value to FatPointer".to_string()}),
        }
    }
}


/// A hand-rolled string representation: a raw pointer to bytes, the length, and
/// a cached hash.
///
/// ## Why this exists
/// This is a direct port of clox's `ObjString`, which stores `char* chars`,
/// `int length`, and a precomputed `hash`. The hash is cached so string-keyed
/// hash tables (globals, the string-intern table) don't rehash on every lookup.
/// "Fat pointer" = a pointer bundled with extra metadata (here, size + hash).
///
/// ## Why this is NOT how you'd normally do it in Rust
/// Rust already has owned/shared string types — `String`, `Box<str>`, `Rc<str>`
/// — that carry their length, free their memory automatically, and compare by
/// *content*. This `FatPointer`:
///   - uses a raw `*mut u8` that Rust does not track, so nothing frees it
///     (a memory leak) and misusing it is `unsafe`;
///   - compares by *pointer identity*, not content (see `PartialEq` below).
/// It exists here to learn manual memory management the way clox does it, not
/// because it is the idiomatic choice.
#[derive(Debug, Clone)]
pub(crate) struct FatPointer {
    /// Raw pointer to the first byte of the string data. Not owned/tracked by
    /// Rust — see `memory.rs` for how it is allocated.
    pub(crate) ptr: *mut u8,
    /// Number of bytes the string occupies.
    pub(crate) size: usize,
    /// Precomputed hash of the contents (see `hasher.rs`), cached to avoid
    /// rehashing on every table lookup.
    pub(crate) hash: u32,
}


impl PartialEq for FatPointer {
    fn eq(&self, other: &Self) -> bool {
        // Equality is pointer identity: two `FatPointer`s are equal iff they
        // point at the same address. This is correct because every string is
        // interned to one canonical pointer — both literals (at compile time)
        // and runtime results like `concat` (which now interns), so equal
        // content always shares one pointer.
        // BUG: the `matches!(self, _other)` guard below is always true (`_other`
        // is a catch-all binding, not a comparison) — dead code that could be
        // removed; the real logic is the `ptr ==` line.
        if matches!(self, _other) {
           return self.ptr == other.ptr;
        }
        false
    }
}

/// A compiled Lox function (also used for the top-level script and closures).
///
/// This is the compiler's output unit: a function owns its own `Chunk` of
/// bytecode. Functions are heap objects at runtime (`Obj::Fun`).
#[derive(Debug, Clone)]
pub(crate) struct Function {
    /// Number of parameters the function declares. Checked against the argument
    /// count at call time.
    pub(crate) arity: u8,
    /// The function's compiled bytecode + constants + line info.
    pub(crate) chunk: Chunk,
    /// The function's name, or `None` for the top-level script.
    pub(crate) name: Option<FatPointer>,
    /// Whether this is the script, a named function, or a closure.
    pub(crate) func_type: FunctionType,
}

impl Function {
    /// Create an empty function of the given type with a fresh, empty chunk.
    pub(crate) fn new_function(fun_type: FunctionType) -> Function {
        Function {
            arity: 0,
            chunk: Chunk::init(),
            name: None,
            func_type: fun_type,
        }
    }
}

/// Distinguishes the kinds of callable objects.
#[derive(Debug, Clone)]
pub(crate) enum FunctionType {
    /// A normal named function.
    Function,
    /// The implicit top-level "function" wrapping the whole script.
    Script,
    /// A function that captures variables from an enclosing scope.
    Closure,
}

/// A heap-allocated object.
///
/// `Value::Number`/`Boolean`/`Missing` are small and live inline on the stack.
/// Anything variable-sized or shared lives behind `Obj`. In clox these are all
/// `Obj*` with a type tag; here they are enum variants.
#[derive(Debug, Clone)]
pub(crate) enum Obj {
    /// A string (see `FatPointer`).
    Str(FatPointer),
    /// A compiled function.
    Fun(Function),
    /// A closure. `Box` is a heap allocation holding a single `Obj` — needed
    /// here because an enum cannot directly contain itself (that would be an
    /// infinitely large type); the indirection through `Box` gives it a fixed
    /// size.
    // NOTE: a closure should carry captured upvalues; this only wraps the
    // function, matching the incomplete closure support elsewhere.
    Closure(Box<Obj>),
    /// The absence of an object.
    Nil,
}

impl Obj {
    /// True if this object is a string.
    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self, Obj::Str(_))
    }

    /// True if this object is the nil object.
    #[inline]
    pub fn is_nil(&self) -> bool {
        matches!(self, Obj::Nil)
    }

    /// Borrow the bytecode chunk of a function object mutably.
    ///
    /// The compiler calls this to emit instructions into the function it is
    /// currently building. Panics if the object is not a function.
    pub fn get_func_chunk(&mut self) -> &mut Chunk {
        match self {
            Obj::Fun(function) => &mut function.chunk,
            _ => panic!("Not able to convert to function from object"),
        }
    }
}

impl PartialEq for Obj {
    fn eq(&self, other: &Self) -> bool {
        // BUG: same always-true `matches!(self, _other)` dead guard.
        // NOTE: only strings are comparable, and via `FatPointer`'s
        // pointer-identity equality. Functions/closures always compare unequal.
        if matches!(self, _other) {
            return match (self, other) {
                (Obj::Str(l), Obj::Str(r)) => l == r,
                _ => false,
            };
        }
        false
    }
}

/// Wrap an existing `FatPointer` as a string object. This is a *total*
/// conversion (it cannot fail), which is why `From` is appropriate here.
impl From<FatPointer> for Obj {
    fn from(ptr: FatPointer) -> Self {
        Obj::Str(ptr)
    }
}

/// Build a brand-new string object from a Rust string slice: hash it, allocate
/// raw bytes, copy the contents in, and wrap the pointer.
impl From<&mut str> for Obj {
    fn from(str_value: &mut str) -> Self {
        let hash_value = hasher::hash(str_value);
        // Allocate exactly `str_value.len()` bytes for the text, then copy the
        // contents in. (Previously this called `allocate::<String>()`, which
        // reserved the 24-byte String struct header rather than the text length
        // and overflowed the buffer for longer strings.)
        let str_ptr = memory::allocate_bytes(str_value.len());
        memory::copy(str_value.as_mut_ptr(), str_ptr, str_value.len(), 0);
        let fat_ptr = FatPointer {
            ptr: str_ptr,
            size: str_value.len(),
            hash: hash_value,
        };
        Obj::from(fat_ptr.clone())
    }
}

impl TryFrom<Obj> for FatPointer {
    type Error = ConversionError;
    fn try_from(obj: Obj) -> Result<FatPointer, Self::Error> {
        match obj {
            Obj::Str(ptr) => Ok(ptr),
            // BUG: on a non-string this fabricates a `FatPointer` from
            // `"".to_string().as_mut_ptr()` — a pointer into a temporary
            // `String` that is dropped at the end of this expression, leaving a
            // dangling pointer. Even for an empty string this is unsound.
            _ => Err(ConversionError{message: "Can not get FatPointer".to_string()}),
        }
    }
}

impl TryFrom<Obj> for Function {
    type Error = ConversionError;
    fn try_from(obj: Obj) -> Result<Function, Self::Error> {
        match obj {
            Obj::Fun(function) => Ok(function),
            _ => Err(ConversionError{message: "Can not get Function".to_string()}),
        }
    }
}


/// Pick a random RGB terminal color. Used only by the debug output to tint each
/// call frame differently so nested calls are easy to tell apart on screen.
pub(crate) fn random_color() -> Color {
    let r: u8 = rand::thread_rng().gen_range(1..=255);
    let g: u8 = rand::thread_rng().gen_range(1..=255);
    let b: u8 = rand::thread_rng().gen_range(1..=255);
    Color::TrueColor { r, g, b }
}
