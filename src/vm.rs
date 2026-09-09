//! # The Virtual Machine (VM) — the execution engine
//!
//! This is the last stage of the pipeline: `source text -> scanner -> compiler
//! -> bytecode -> **VM**`. The compiler produced a `Chunk` (a flat `Vec<u8>` of
//! bytecode plus a pool of constants). The VM's job is to *execute* that
//! bytecode.
//!
//! ## Core idea: a stack-based bytecode interpreter
//!
//! Rather than walking a tree of syntax nodes (a "tree-walking interpreter",
//! which is slow), we compiled the program down to a linear sequence of
//! one-byte **opcodes**. The VM runs a single tight loop that repeatedly:
//!   1. reads the next opcode byte (advancing the *instruction pointer*),
//!   2. `match`es on it, and
//!   3. performs the corresponding action.
//! This loop is called the **dispatch loop** (see [`VM::run`]).
//!
//! Almost every operation communicates through a single **value stack**. For
//! example `1 + 2` compiles to: push 1, push 2, Add (which pops two, pushes the
//! sum). This "operand stack" model is why it's called a *stack-based* VM — it
//! mirrors how a real CPU uses a stack, and makes code generation trivial
//! because every expression just leaves its result on top of the stack.
//!
//! This file mirrors the C `vm.c` from *Crafting Interpreters* (the "clox"
//! bytecode VM), translated to Rust.

extern crate num;

use crate::common::{random_color, FatPointer, Function, Obj, OpCode, Value};
use crate::debug;
use crate::hash_map::{Table, Entry};
use crate::hasher::hash;
use crate::metrics;
use crate::scanner::Scanner;
use crate::{compiler, memory};
use colored::{Color, Colorize};

/// Upper bound on the value stack. Fixed-size stacks are traditional in VMs
/// because they make pushes O(1) with no reallocation and no bounds surprises;
/// the tradeoff is a hard recursion/expression-depth limit.
const STACK_MAX: usize = 512;

/// The virtual machine: holds all runtime state while executing bytecode.
#[derive(Debug)]
pub(crate) struct VM {
    /// Legacy top-level instruction pointer. NOTE: the *real* instruction
    /// pointer used during execution lives per-`CallFrame` (`CallFrame::ip`);
    /// this field is essentially vestigial (only set in `interpret`).
    ip: i32,
    /// The value stack. Every operand and intermediate result lives here.
    /// Pre-filled with `None` slots up to `STACK_MAX` so we can index directly
    /// instead of pushing/popping a growable `Vec` (mirrors clox's raw array).
    stack: Vec<Option<Value>>,
    /// Index of the next free slot — i.e. one past the current top of stack.
    /// This is the "stack pointer". `push` writes here then increments;
    /// `pop` decrements then reads.
    stack_top: usize,
    /// Interned string table (string interning: keep one canonical copy of
    /// each string so identical literals share storage). Shared with the
    /// compiler during compilation.
    table: Table<Value>,
    /// Global variables, keyed by their (interned) name.
    globals: Table<Value>,
    /// The call stack: one `CallFrame` per active function invocation.
    /// Pre-sized with `None` like the value stack.
    call_frames: Vec<Option<CallFrame>>,
    /// Number of active call frames — index of the next free frame slot.
    frame_count: usize,
}

/// One activation record for a running function call.
///
/// Core idea: each function call needs its own instruction pointer and its own
/// region of the value stack. A `CallFrame` records where that function's
/// bytecode is (`function`/`ip`) and where its slice of the shared value stack
/// begins (`cf_stack_top`). Local variables are then addressed *relative* to
/// that base, which is what makes the same function reusable across recursive
/// calls.
#[derive(Debug, Clone)]
pub(crate) struct CallFrame {
    /// The function being executed (owns the `Chunk` of bytecode + constants).
    function: Function,
    /// Instruction pointer: index of the next bytecode byte to read *within
    /// this frame's chunk*. This is the pointer the dispatch loop actually
    /// advances.
    ip: usize,
    /// Base of this frame's window into the shared value stack. Local slot `n`
    /// lives at `stack[cf_stack_top + n]`. Slot 0 is reserved for the callee
    /// itself (the function/closure object), which is why parameters start at
    /// slot 1.
    cf_stack_top: usize,
    /// Purely cosmetic: a random color used to tint this frame's debug output
    /// so nested calls are visually distinguishable in traces.
    color: Color,
}

impl CallFrame {
    /// Debug helper: print this frame's function name (or "Main" for the
    /// top-level script, whose function has no name), tinted with the frame's
    /// color. Reads the name out of manually-managed memory via `FatPointer`.
    fn print_name(&self) {
        match &self.function.name {
            Some(ptr) => {
                let cf_name = memory::read_string(ptr.ptr, ptr.size);
                println!(
                    "{}",
                    format!("****** CallFrame: {:?} ******", cf_name)
                        .color(self.color)
                        .bold()
                );
            }
            None => println!(
                "{}",
                format!("****** CallFrame: {:?} ******", "Main")
                    .color(self.color)
                    .bold()
            ),
        }
    }
}

/// Outcome of running a program. The caller (`main`/REPL) uses this to decide
/// process exit codes. Mirrors clox's `InterpretResult`.
pub enum InterpretResult {
    InterpretOk,
    InterpretCompileError,
    InterpretRuntimeError,
}

/// Read the next bytecode byte and advance the frame's instruction pointer.
///
/// This is the fundamental primitive of the dispatch loop. It is a macro rather
/// than a method because it needs to mutate `$frame.ip` in place at the call
/// site while the surrounding code also borrows `$self` — a plain method taking
/// `&mut self` would create borrow-checker conflicts. Macros paste code
/// textually, sidestepping the borrow analysis. (clox uses a C `#define` here
/// for the same reason plus speed.)
macro_rules! READ_BYTE {
    ($self:ident, $frame:ident) => {
        *{
            let c = $frame.function.chunk.code.get($frame.ip as usize);
            $frame.ip += 1;
            c.unwrap()
        }
    };
}

/// Read the next byte as an index into the chunk's constant pool and return
/// that constant. Used to load literals (numbers, strings, functions) that were
/// too big to encode directly in the bytecode stream.
macro_rules! READ_CONSTANT {
    ($self:ident, $frame:ident) => {{
        let index = READ_BYTE!($self, $frame) as usize;
        debug::info(format!("Reading constant from index: {:?}", index));
        $frame.function.chunk.constants.values.get(index)
    }};
}

/// Emit a numeric binary operation (`+ - * / < >`).
///
/// The stack discipline: the two operands are already on top of the stack (the
/// left operand was pushed first, so it sits *below* the right). We type-check
/// both are numbers, pop the pair, apply the Rust operator `$op`, and push the
/// result. `$op:tt` is a "token tree" — it lets us pass a raw operator token
/// like `+` into the macro and splice it directly into the expression.
///
/// NOTE: on a type error this `return`s from the enclosing function, which is
/// why it must be a macro (a helper method could not force the caller to
/// return).
macro_rules! BINARY_OP {
    ($self:ident, $op:tt) => {{
        let peek_0 = $self.peek(0).as_ref().unwrap();
        let peek_1 = $self.peek(1).as_ref().unwrap();
        if !peek_0.is_number() || !peek_1.is_number() {
            $self.runtime_error("Expected two numbers for binary operation.");
            return InterpretResult::InterpretRuntimeError;
        }
        // pop_pair returns (top, second) = (right operand, left operand)
        let (right_val_popped, left_val_popped)  = $self.pop_pair();
        let left_float_val = Into::<f64>::into(left_val_popped.as_ref().unwrap());
        let right_float_val = Into::<f64>::into(right_val_popped.as_ref().unwrap());
        $self.push(Value::from(left_float_val $op right_float_val));
    }}
}

/// Like `READ_CONSTANT`, but reads an 8-byte (`usize`) index instead of one
/// byte. Used for the `ConstantLong` opcode, which lets a chunk hold more than
/// 256 constants (a single-byte index caps out at 255).
macro_rules! READ_CONSTANT_LONG {
    ($self:ident, $frame:ident) => {{
        let mut constant_index_bytes = [0, 0, 0, 0, 0, 0, 0, 0];
        // our long constant index is usize which is 8 bytes
        for i in 1..=8 {
            constant_index_bytes[i - 1] = READ_BYTE!($self, $frame);
        }
        let constant_index = usize::from_ne_bytes(constant_index_bytes);
        $frame
            .function
            .chunk
            .constants
            .values
            .get(constant_index as usize)
    }};
}

impl VM {
    /// Construct a fresh VM with an empty (but pre-allocated) value stack and
    /// call-frame array, and empty string/global tables.
    pub(crate) fn init() -> VM {
        let mut local_stack = Vec::with_capacity(STACK_MAX);

        for _i in 0..STACK_MAX {
            local_stack.push(None);
        }

        let mut call_frames: Vec<Option<CallFrame>> = Vec::new();
        call_frames.resize(512, None);

        VM {
            ip: -1,
            stack: local_stack,
            stack_top: 0,
            table: Table::init(10),
            globals: Table::init(10),
            call_frames,
            frame_count: 0,
        }
    }

    /// Clear the stack by resetting the stack pointer to the bottom. (Old
    /// values are left in place but become unreachable — `stack_top` is the
    /// only source of truth for what's "on" the stack.)
    fn reset_stack(&mut self) {
        self.stack_top = 0;
    }

    /// Push a value: write it at the top slot, then bump the stack pointer.
    fn push(&mut self, value: Value) {
        self.stack[self.stack_top] = Option::Some(value);
        self.stack_top += 1;
    }

    /// Pop a value: move the stack pointer down, then return the slot it now
    /// points at. Returns a reference (the slot is not cleared).
    fn pop(&mut self) -> &Option<Value> {
        self.stack_top -= 1;
        self.stack.get(self.stack_top).unwrap()
    }

    /// Pop the top two values at once, returning `(top, second)`.
    ///
    /// Ordering matters: after decrementing by 2, `stack_top + 1` is the former
    /// top of stack and `stack_top` is the one below it. For a binary operator
    /// the left operand was pushed first (lower), the right operand second
    /// (higher/top) — so this returns `(right, left)`. Callers rely on that
    /// order.
    fn pop_pair(&mut self) -> (&Option<Value>, &Option<Value>) {
        self.stack_top -= 2;
        (self.stack.get(self.stack_top + 1).unwrap(), self.stack.get(self.stack_top).unwrap())
    }

    /// Look at a value without removing it. `distance` is measured from the top:
    /// `peek(0)` is the top of stack, `peek(1)` the one below, etc. Used to
    /// inspect operands' types before committing to popping them.
    fn peek(&self, distance: usize) -> &Option<Value> {
        self.stack.get(self.stack_top - 1 - distance).unwrap()
    }


    /// Report a runtime error.
    ///
    /// NOTE: this currently only *logs* the message via the debug channel — it
    /// does not unwind the stack or print a proper stack trace. Callers must
    /// themselves `return InterpretResult::InterpretRuntimeError` to actually
    /// abort execution; calling this alone does not stop the VM.
    fn runtime_error(&self, message: &str) {
        debug::info(format!("Runtime error: {:?}", message));
    }

    /// The heart of the VM: the **dispatch loop**.
    ///
    /// Repeatedly read one opcode, decode it (`from_u8` turns the raw byte back
    /// into an `OpCode`), and execute it. This runs until a `Return` from the
    /// top-level frame, or a runtime error.
    ///
    /// Implementation note: it keeps a *clone* of the currently-executing
    /// `CallFrame` in `current_frame` and operates on that, writing it back to
    /// `self.call_frames` when switching frames (on `Call`/`Return`). Cloning
    /// the frame each switch is a workaround for Rust's borrow checker (we can't
    /// hold a `&mut` into `self.call_frames` while also calling `&mut self`
    /// methods like `push`/`pop`); it costs a copy per call but keeps the
    /// borrows simple.
    fn run(&mut self) -> InterpretResult {
        let mut current_frame = self.call_frames[self.frame_count - 1]
            .as_ref()
            .unwrap()
            .clone();
        loop {
            let instruction = READ_BYTE!(self, current_frame);
            // Decode the raw byte into an OpCode. `None` => unknown/unsupported
            // opcode, which falls through to the catch-all arm below.
            let opcode = num::FromPrimitive::from_u8(instruction);
            self.print_debug_info(&mut current_frame, &instruction, &opcode);

            match opcode {
                Some(OpCode::Return) => {
                    // Pop this call's return value, tear down its frame. If it
                    // was the top-level frame we're done; otherwise resume the
                    // caller by reloading it into `current_frame`.
                    let is_last_frame = self.return_op(&mut current_frame);
                    if is_last_frame {
                        return InterpretResult::InterpretOk;
                    }
                    current_frame = self.call_frames[self.frame_count - 1]
                        .as_ref()
                        .unwrap()
                        .clone();
                }
                Some(OpCode::Negate) => {
                    // Unary minus: peek to type-check, then pop and push -x.
                    let value = self.peek(0).as_ref().unwrap();
                    if !value.is_number() {
                        self.runtime_error("Expected number for Negate opcode!");
                        return InterpretResult::InterpretRuntimeError;
                    }
                    let pop_val = self.pop().as_ref().unwrap();
                    let float_val = Into::<f64>::into(pop_val);
                    self.push(Value::from(-1.0 * float_val));
                }
                Some(OpCode::Add) => {
                    // `+` is overloaded: string concatenation OR numeric add.
                    // We peek at the top operand to decide which path to take.
                    let value = self.peek(0).as_ref().unwrap();
                    match value {
                        Value::Obj(obj) => {
                            if obj.is_string() {
                                if self.peek(1).as_ref().unwrap().is_obj_string() {
                                    let combined = self.concat();
                                    self.push(combined);
                                }
                            } else {
                                self.runtime_error("Expected String value on right side while adding to another string.");
                                return InterpretResult::InterpretRuntimeError;
                            }
                        }
                        Value::Number(_value) => BINARY_OP!(self, +),
                        _ => {
                            // NOTE: this reports an error but returns
                            // `InterpretOk` (not `InterpretRuntimeError`), so a
                            // bad Add is silently treated as success.
                            self.runtime_error("Unknown type detected for Add operation");
                            return InterpretResult::InterpretOk;
                        }
                    }
                }
                // Numeric binary operators. `>` and `<` produce a Boolean;
                // `>=`/`<=`/`==`/`!=` are compiled as combinations (e.g. `>=`
                // becomes `Less` followed by `Not`), so there are no dedicated
                // opcodes for them here.
                Some(OpCode::Multiply) => {
                    BINARY_OP!(self, *);
                }
                Some(OpCode::Subtract) => {
                    BINARY_OP!(self, -);
                }
                Some(OpCode::Divide) => {
                    BINARY_OP!(self, /);
                }
                Some(OpCode::Greater) => {
                    BINARY_OP!(self, >);
                }
                Some(OpCode::Less) => {
                    BINARY_OP!(self, <);
                }
                Some(OpCode::Equal) => {
                    // Pop both operands and compare with `Value`'s PartialEq.
                    // NOTE: for strings this compares by pointer identity, so
                    // two equal-but-separately-allocated strings (e.g. the
                    // result of a runtime concat) compare as NOT equal.
                    let (left, right) = self.pop_pair();
                    let is_equal = left.as_ref().unwrap() == right.as_ref().unwrap();
                    self.push(Value::from(is_equal));
                }
                Some(OpCode::Constant) => {
                    // Load a literal from the constant pool (1-byte index) onto
                    // the stack.
                    let constant = READ_CONSTANT!(self, current_frame);
                    self.push((*constant.unwrap()).clone());
                }
                Some(OpCode::False) => {
                    self.push(Value::from(false));
                }
                Some(OpCode::True) => {
                    self.push(Value::from(true));
                }
                Some(OpCode::Nil) => {
                    self.push(Value::Missing);
                }
                Some(OpCode::Not) => {
                    // Logical `!`: pop, and push the truthiness-negated result.
                    let value = self.pop().as_ref().unwrap().clone();
                    self.push(Value::from(self.is_falsey(value)));
                }
                Some(OpCode::ConstantLong) => {
                    // Same as Constant but with an 8-byte index (>255 constants).
                    let constant = READ_CONSTANT_LONG!(self, current_frame);
                    self.push((*constant.unwrap()).clone());
                }
                Some(OpCode::DefineGlobalVariable) => {
                    // Bind a global: the name is a constant, the value is on top
                    // of the stack. Insert into `globals`, then pop the value.
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = Into::<FatPointer>::into(&constant);
                    let value = self.peek(0).as_ref().unwrap();
                    debug::info(format!(
                        "DefineGlobalVariable: Define constant value: {:?}",
                        value
                    ));
                    self.globals.insert(variable_name, value.clone());
                    self.pop();
                }
                Some(OpCode::Pop) => {
                    // Discard the top of stack — emitted after statements and
                    // to clean up branch conditions.
                    self.pop();
                }
                Some(OpCode::Closure) => {
                    // Wrap a compiled function into a runtime closure and push
                    // it. BUG/INCOMPLETE: the compiler emits upvalue operand
                    // bytes (is_local + index pairs) right after the Closure
                    // instruction, but this arm never reads them. So `ip` is
                    // left pointing at those operand bytes, which then get
                    // misinterpreted as opcodes. Closures that capture
                    // variables therefore do not work.
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let function_obj = Into::<Obj>::into(&constant);
                    let closure = Obj::Closure(Box::new(function_obj));
                    self.push(Value::from(closure));
                }
                Some(OpCode::Call) => {
                    // Read how many arguments were passed, then set up a new
                    // frame for the callee. `execute_function` pushes the new
                    // frame; we then reload `current_frame` to the callee.
                    let arg_count = READ_BYTE!(self, current_frame);
                    let old_frame = current_frame.clone();
                    if !self.execute_function(arg_count as usize, arg_count) {
                        return InterpretResult::InterpretRuntimeError;
                    }
                    current_frame = self.call_frames[self.frame_count - 1]
                        .as_ref()
                        .unwrap()
                        .clone();
                    // Save the caller's frame (with its advanced `ip`) back into
                    // the array. It's at `frame_count - 2` because the callee
                    // we just created occupies `frame_count - 1`.
                    self.call_frames[self.frame_count - 2] = Some(old_frame);
                }
                Some(OpCode::JumpIfFalse) => {
                    // Conditional jump used for `if`/`while`/`and`/`or`. The
                    // condition value is left on the stack (peeked, not popped —
                    // the compiler emits an explicit Pop). If falsey, jump
                    // forward by the 2-byte offset; otherwise skip over those 2
                    // operand bytes and fall through.
                    if self.is_falsey(self.peek(0).as_ref().unwrap().clone()) {
                        //current_frame.ip += offset as usize;
                        current_frame = self.update_offset(current_frame, true);
                    } else {
                        current_frame.ip = current_frame.ip + 2;
                    }
                }
                Some(OpCode::Jump) => {
                    // Unconditional forward jump (e.g. over an `else` branch).
                    current_frame = self.update_offset(current_frame, true);
                }
                Some(OpCode::Loop) => {
                    // Backward jump: same 2-byte offset, but subtracted from
                    // `ip` to loop back to the start of a `while`/`for` body.
                    current_frame = self.update_offset(current_frame, false);
                }
                Some(OpCode::GetLocalVariable) => {
                    // Locals are just stack slots. Read the 1-byte slot number
                    // and copy that slot (relative to this frame's base) onto
                    // the top of the stack.
                    let b = READ_BYTE!(self, current_frame);
                    let val = self.stack[current_frame.cf_stack_top + b as usize]
                        .clone()
                        .unwrap();
                    self.push(val.clone());
                }
                Some(OpCode::SetLocalVariable) => {
                    // Assign to a local: write the current top-of-stack value
                    // into the local's slot. Peeks (does not pop) because
                    // assignment is an expression whose value stays on the stack.
                    let b = READ_BYTE!(self, current_frame);
                    self.stack[current_frame.cf_stack_top + b as usize] = Some(self.peek(0).as_ref().unwrap().clone());
                }
                Some(OpCode::GetGlobalVariable) => {
                    // Read a global by name (name is a constant) and push its
                    // value; error if the name is unbound.
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    debug::info(format!(
                        "GetGlobalVariable: Read constant value: {:?}",
                        constant
                    ));
                    let variable_name = Into::<FatPointer>::into(&constant);
                    if let Some(ret) = self.push_obj_value_to_stack(variable_name) {
                        return ret;
                    }
                }
                Some(OpCode::SetGlobalVariable) => {
                    // Assign to an existing global; error if it wasn't declared.
                    let constant = READ_CONSTANT!(self, current_frame).unwrap().clone();
                    let variable_name = Into::<FatPointer>::into(&constant);
                    if let Some(ret) = self.set_global_variable(variable_name) {
                        return ret;
                    }
                }
                Some(OpCode::Print) => {
                    // `print` statement: pop the value and display it.
                    debug::print_value(self.pop().as_ref().unwrap(), true);
                }
                _ => {
                    // Catch-all for opcodes with no arm above.
                    // BUG/INCOMPLETE: `GetUpValue` and `SetUpValue` are emitted
                    // by the compiler but have no arm here, so they land in this
                    // branch — which *silently halts the VM and returns Ok*
                    // rather than executing them. Combined with the `Closure`
                    // operand-byte bug, this is why closures over captured
                    // variables don't run.
                    debug::info(format!("Stopping vm: {:?}", opcode));
                    self.call_frames[self.frame_count - 1] = Some(current_frame);
                    return InterpretResult::InterpretOk;
                }
            }
        }
    }

    /// Assign to an *existing* global (the `x = ...` form, not `var x = ...`).
    ///
    /// Lox forbids assigning to an undeclared global. The check here is
    /// indirect: `insert` returns whether the key already existed. If it did
    /// not, this was an assignment to an undefined variable, so we undo the
    /// insert (delete it) and raise a runtime error. Returns `Some(err)` to
    /// signal the caller to abort, or `None` on success.
    fn set_global_variable(&mut self, variable_name: FatPointer) -> Option<InterpretResult> {
        let size = variable_name.size;
        let ptr = variable_name.ptr;
        let value = self.peek(0);

        if !self.globals.insert(variable_name.clone(), value.as_ref().unwrap().clone()) {
            self.globals.delete(variable_name.clone());
            let key = memory::read_string(ptr, size);
            let message = format!("Unable to find value for key {:?}", key);
            self.runtime_error(message.as_str());
            return Some(InterpretResult::InterpretRuntimeError);
        }

        None
    }

    /// Read a global's value and push it onto the stack (the `GetGlobalVariable`
    /// path). If the name isn't bound, raise a runtime error and return
    /// `Some(err)`.
    ///
    /// The large `match` on the value's type is purely for debug logging — every
    /// branch does the same `self.push(val.clone())`; only the log message
    /// differs.
    fn push_obj_value_to_stack(&mut self, variable_name: FatPointer) -> Option<InterpretResult> {
        let size = variable_name.size;
        let ptr = variable_name.ptr;
        let value = self.get_variable_value(variable_name);
        debug::info(format!(
            "Found global value: {:?}",
            value
        ));
        match value {
            Some(val) => match value {
                Some(Value::Boolean(v)) => {
                    debug::info(format!("Boolean value pushing to stack {:?}", v));
                    self.push(val.clone());
                }
                Some(Value::Number(v)) => {
                    debug::info(format!("Number value pushing to stack {:?}", v));
                    self.push(val.clone());
                }
                Some(Value::Obj(obj)) => match obj {
                    Obj::Str(ptr) => {
                        let c_value = memory::read_string(ptr.ptr, ptr.size);
                        debug::info(format!(
                            "String Object value pushing to stack {:?}",
                            c_value
                        ));
                        self.push(val.clone());
                    }
                    Obj::Fun(function) => {
                        let function_name = function.name.as_ref().unwrap();
                        let name = memory::read_string(function_name.ptr, function_name.size);
                        debug::info(format!(
                            "Function Object value pushing to stack {:?} with name: {:?}",
                            function, name
                        ));
                        self.push(val.clone());
                    }
                    _ => {
                        debug::info(format!("Unknown object pushing to stack {:?}", obj));
                        self.push(val.clone());
                    }
                },
                _ => {
                    debug::info(format!("Unknown value pushing to stack"));
                    self.push(val.clone());
                }
            },
            None => {
                let key = memory::read_string(ptr, size);
                let message = format!("Unable to find value for key {:?}", key);
                self.runtime_error(message.as_str());
                return Some(InterpretResult::InterpretRuntimeError);
            }
        }
        None
    }

    /// Trace helper: print the current frame name, optionally the whole stack,
    /// and disassemble the instruction about to run. Called once per dispatch
    /// loop iteration; controlled by the flags in `debug`.
    fn print_debug_info(
        &mut self,
        current_frame: &mut CallFrame,
        instruction: &u8,
        opcode: &Option<OpCode>,
    ) {
        current_frame.print_name();
        if !matches!(opcode, None) {
            if debug::PRINT_STACK {
                debug::info(format!("##### Stack[Start] ###### \n"));
                for i in 0..self.stack.len() {
                    print!("[{:?}] ", self.stack[i]);
                }
                debug::info(format!("\n\n ##### Stack[End] ######"));
            }

            current_frame
                .function
                .chunk
                .handle_instruction(&instruction, (current_frame.ip - 1) as usize);
        }
    }

    /// Handle `return`. Returns `true` if this was the top-level frame (program
    /// is finished), `false` if control should resume in the caller.
    ///
    /// Steps: pop the return value; drop this frame (`frame_count -= 1`). If a
    /// caller remains, discard the callee's entire stack window by resetting
    /// `stack_top` to `cf_stack_top` (which also removes the callee object and
    /// its arguments), then push the return value so the caller finds it on top.
    fn return_op(&mut self, current_frame: &mut CallFrame) -> bool {
        let result = self.pop().as_ref().unwrap().clone();
        self.frame_count -= 1;

        if self.frame_count == 0 {
            // @todo check if we need this pop.
            //self.pop();
            return true;
        }
        // Collapse the stack back to where this frame began, wiping the callee
        // and its arguments in one move.
        self.stack_top = current_frame.cf_stack_top;
        debug::info(format!("Pushing return value to stack: {:?}", result));
        self.push(result);
        false
    }

    /// Set up a call: find the callee on the stack (at `peek(distance)`, i.e.
    /// below its arguments), verify it's callable, and create its `CallFrame`.
    /// Returns `false` if the callee isn't a function/closure.
    ///
    /// NOTE: the arity check reports a mismatch via `runtime_error` but does
    /// *not* return `false` / abort — it falls through and creates the frame
    /// anyway. So calling a function with the wrong number of arguments is
    /// logged but not actually prevented.
    fn execute_function(&mut self, distance: usize, arg_count: u8) -> bool {
        let callee = self.peek(distance);
        if callee.as_ref().unwrap().is_obj() {
            let obj = Into::<Obj>::into(callee.as_ref().unwrap());
            match obj {
                Obj::Fun(function) => {
                    if function.arity != arg_count {
                        self.runtime_error(
                            format!(
                                "Expected: {:?} arguments but received: {:?}",
                                function.arity, arg_count
                            )
                            .as_str(),
                        );
                    }
                    self.create_call_frame(function, arg_count);
                    return true;
                }
                Obj::Closure(obj) => {
                    let function = Into::<Function>::into(*obj);
                    if function.arity != arg_count {
                        self.runtime_error(
                            format!(
                                "Expected: {:?} arguments but received: {:?}",
                                function.arity, arg_count
                            )
                            .as_str(),
                        );
                    }
                    self.create_call_frame(function, arg_count);
                    return true;
                }
                _ => (),
            }
        }
        println!("Expected function but instead got: {:?}", callee);
        self.runtime_error("Can only execute function");
        false
    }

    /// Push a new `CallFrame` for `function`, computing its stack window base.
    ///
    /// The base (`cf_stack_top`) is set so that slot 0 of the window lands on
    /// the callee object itself and the arguments occupy the slots just above
    /// it: `stack_top - arg_count - 1`. That `-1` is the reserved slot-0 the
    /// compiler always leaves for the function (later used for `this` on
    /// methods).
    fn create_call_frame(&mut self, function: Function, arg_count: u8) {
        let mut cf_stack_top = 0;
        if self.stack_top > 0 {
            /*
             * The funny little - 1 is to account for stack slot zero which the compiler
             * set aside for when we add methods later.
             * The parameters start at slot one so we make the window start
             * one slot earlier to align them with the arguments.
             * -1 is for name of the function
             */
            cf_stack_top = self.stack_top - (arg_count as usize) - (1 as usize);
        };

        let call_frame = CallFrame {
            function,
            ip: 0, //@todo check if this value should be 0 or not
            cf_stack_top,
            color: random_color(),
        };
        self.call_frames[self.frame_count] = Some(call_frame);
        println!("Callframes SIZE: {:?}", self.call_frames.iter().filter(|cf| matches!(cf, Some(_))).count());
        self.frame_count += 1;
    }

    /// Read a 2-byte jump offset at the current `ip` and move `ip` by it.
    ///
    /// Two-byte offsets: a jump distance is stored as two bytes so it can span
    /// up to 65535 bytes of code (one byte would cap at 255). Here the bytes are
    /// assembled into a `u16`. `add == true` jumps forward (if/while/else/`and`/
    /// `or`); `add == false` jumps backward (loop). `ip` is first advanced past
    /// the 2 operand bytes, *then* the offset is applied.
    ///
    /// NOTE the byte order: `offset_bytes[0]` is taken from `ip + 1` and
    /// `offset_bytes[1]` from `ip`, and it's decoded with `from_ne_bytes`
    /// (native-endian). This must stay consistent with however the compiler
    /// wrote the two bytes.
    fn update_offset(&self, mut current_frame: CallFrame, add: bool) -> CallFrame {
        let offset_bytes: [u8; 2] = [
            current_frame.function.chunk.code[(current_frame.ip + 1) as usize],
            current_frame.function.chunk.code[(current_frame.ip) as usize],
        ];
        current_frame.ip = current_frame.ip + 2;
        // adding 2 because we read offset bytes
        let offset = u16::from_ne_bytes(offset_bytes);
        if add {
            current_frame.ip += offset as usize;
        } else {
            current_frame.ip -= offset as usize;
        }

        current_frame
    }

    /// Look up a global's value by name in the `globals` table.
    fn get_variable_value(&self, variable_name: FatPointer) -> Option<&Value> {
        debug::info(format!(
            "Get variable value for key: {:?}",
            variable_name
        ));
        self.globals.get(variable_name)
    }

    /// Value equality (currently unused helper). Defers to `Value`'s PartialEq.
    fn is_equal(&self, left: Value, right: Value) -> bool {
        left == right
    }

    /// Lox truthiness: only `nil` (`Missing`) and `false` are falsey;
    /// everything else (including 0 and "") is truthy. This is the rule used by
    /// `if`, `while`, `and`, `or`, and `!`.
    fn is_falsey(&self, value: Value) -> bool {
        value.is_missing() || (value.is_boolean() && !Into::<bool>::into(&value))
    }

    /// Concatenate the top two string operands into a new string value.
    ///
    /// Pops the pair, copies both byte ranges into a freshly `allocate`d buffer
    /// via the manual-memory helpers, and returns a new `Obj::Str`.
    /// NOTE: the result is a brand-new, un-interned allocation, which is why
    /// concatenated strings don't compare equal under `OpCode::Equal` (that
    /// path compares string pointers, not contents).
    fn concat(&mut self) -> Value {
        let(second_val, first_val) = self.pop_pair();

        let second = Into::<FatPointer>::into(second_val.as_ref().unwrap());
        let first = Into::<FatPointer>::into(first_val.as_ref().unwrap());

        let total = first.size + second.size;
        let ptr = memory::allocate_bytes(total);
        memory::copy(first.ptr, ptr, first.size, 0);
        memory::copy(second.ptr, ptr, second.size, first.size);

        let hash_value = hash(memory::read_string(ptr, first.size + second.size).as_str());
        Value::from(Obj::from(FatPointer {
            ptr,
            size: (first.size + second.size),
            hash: hash_value,
        }))
    }

    /// Top-level entry point: compile `source` to bytecode, then run it.
    ///
    /// Pipeline in miniature: build a `Scanner`, hand it to the `Compiler`
    /// (sharing our string-interning `table`), compile to a top-level
    /// `Function`, bail out on compile error, otherwise wrap the script function
    /// as a closure, push it (occupying reserved slot 0), create its call frame,
    /// and enter the dispatch loop. The `metrics::record` calls just time the
    /// compile and run phases.
    pub(crate) fn interpret<'m>(&mut self, source: String) -> InterpretResult {
        let chars: Vec<char> = source.chars().collect();
        let scanner = Scanner::init(0, 0, chars);

        let mut compiler = compiler::Compiler::init(scanner, &mut self.table);

        let (had_error, function_obj) = metrics::record("Compiler time".to_string(), || {
            compiler.compile(source.clone())
        });

        if had_error {
            return InterpretResult::InterpretCompileError;
        }
        self.ip = 0;

        self.push(Value::from(Obj::Closure(Box::new(function_obj.clone()))));
        let function = Into::<Function>::into(function_obj);
        debug::info(format!("Main function: {:?}", function.clone()));
        self.create_call_frame(function, 0);
        metrics::record("VM run time".to_string(), || self.run())
    }
}
