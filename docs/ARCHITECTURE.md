# rlox — Architecture & Status

`rlox` is a **bytecode virtual machine for the Lox language**, written in Rust. It follows
the `clox` design from Part III of Robert Nystrom's *Crafting Interpreters* — but translated
into Rust, deliberately keeping the low-level, manual-memory style of the C original so the
project doubles as a way to learn VM internals, language implementation, and Rust at once.

Unlike a tree-walking interpreter, rlox **compiles source to bytecode** and then executes that
bytecode on a stack machine. There is no AST: the compiler emits instructions as it parses.

---

## 1. The pipeline

Source text flows through four stages and ends up executing on the VM:

```mermaid
flowchart LR
    SRC["Source text<br/>(first.lox)"] --> SC["Scanner<br/>scanner.rs<br/><i>chars → tokens</i>"]
    SC -->|"one token<br/>at a time"| CO["Compiler / Pratt parser<br/>compiler.rs<br/><i>tokens → bytecode</i>"]
    CO --> CH["Chunk<br/>chunk.rs<br/><i>bytecode + constants + lines</i>"]
    CH --> VM["Virtual Machine<br/>vm.rs<br/><i>stack-based dispatch loop</i>"]
    VM --> OUT["Program output"]
```

Key property: the scanner is **lazy / on-demand**. The compiler pulls the next token only when
it needs it (`advance`), rather than tokenizing the whole file up front. And compilation is
**single-pass** — bytecode is emitted immediately while parsing, so the compiler sometimes has
to emit an instruction *before* it knows an operand it needs (fixed later by *backpatching*).

---

## 2. Module map

How the source files depend on each other:

```mermaid
flowchart TD
    main["main.rs<br/><i>entry point</i>"] --> vm
    vm["vm.rs<br/><i>execution engine</i>"] --> compiler
    vm --> chunk
    vm --> hash_map
    vm --> memory
    vm --> debug
    vm --> metrics
    compiler["compiler.rs"] --> scanner
    compiler --> chunk
    compiler --> hash_map
    compiler --> hasher
    compiler --> memory
    compiler --> common
    chunk["chunk.rs"] --> value
    chunk --> common
    value["value.rs"] --> common
    common["common.rs<br/><i>OpCode, Value, Obj, FatPointer</i>"] --> chunk
    common --> hasher
    common --> memory
    hash_map["hash_map.rs"] --> common
    hash_map --> memory
    scanner["scanner.rs"]
    hasher["hasher.rs"]
    memory["memory.rs<br/><i>unsafe manual allocation</i>"]
    debug["debug.rs"]
    metrics["metrics.rs"]
```

| File | Role |
|------|------|
| `main.rs` | Entry point. Currently hardcoded to run `first.lox` (REPL/CLI scaffolding is disabled). |
| `scanner.rs` | Lexer. Turns source characters into `Token`s (type + source offsets, not owned strings). |
| `compiler.rs` | Single-pass compiler + Pratt parser. Emits bytecode into a `Chunk`. |
| `chunk.rs` | A `Chunk`: the compiled code of one function (bytecode, constant pool, line info) + disassembler. |
| `common.rs` | Core types: `OpCode`, `Value`, `Obj`, `FatPointer`, `Function`, conversions. |
| `value.rs` | `ValueArray` — the constant pool backing a chunk. |
| `vm.rs` | The bytecode interpreter: value stack, call frames, dispatch loop. |
| `hash_map.rs` | Open-addressing hash table (`Table`) for globals and string interning. |
| `hasher.rs` | FNV-1a string hash. |
| `memory.rs` | Manual `unsafe` allocation (`*mut u8` + `Layout`) for string storage. |
| `debug.rs` | Compile-time debug/trace flags and value printing. |
| `metrics.rs` | Simple `Instant`-based timing of compile/run phases. |

---

## 3. Data model — how a Lox value is represented

```mermaid
classDiagram
    class Value {
        <<enum>>
        Boolean(bool)
        Number(f64)
        Obj(Obj)
        Missing
    }
    class Obj {
        <<enum>>
        Str(FatPointer)
        Fun(Function)
        Closure(Box~Obj~)
        Nil
    }
    class FatPointer {
        ptr: *mut u8
        size: usize
        hash: u32
    }
    class Function {
        arity: u8
        chunk: Chunk
        name: Option~FatPointer~
        func_type: FunctionType
    }
    Value --> Obj : wraps
    Obj --> FatPointer : Str / name
    Obj --> Function : Fun
    Function --> FatPointer : name
```

- `Value` is a **tagged union** (Rust enum): every runtime value is a bool, an `f64` number, a
  heap object, or `Missing` (Lox `nil`).
- Strings are **not** Rust `String`s. They're a `FatPointer` — a raw pointer + length + cached
  hash — mirroring clox's `ObjString`. The bytes live in memory hand-allocated by `memory.rs`.
  *(Idiomatic Rust would use `Rc<str>`; the raw-pointer approach is a deliberate exercise.)*
- `Closure` boxes an `Obj` because it refers to a `Function` object (a recursive type needs
  indirection in Rust).

---

## 4. Execution model — the stack machine

The VM is a **stack-based interpreter**. Instructions push/pop operands on a value stack. Each
function call gets a `CallFrame` that owns a *window* into that shared stack.

```mermaid
flowchart TD
    subgraph VM["VM state"]
        direction TB
        STACK["Value stack<br/>(Vec, stack_top pointer)"]
        FRAMES["Call frames<br/>(Vec, frame_count)"]
        GLOBALS["globals: Table"]
        INTERN["table: interned strings"]
    end

    subgraph FRAME["A CallFrame"]
        direction TB
        FN["function (bytecode + constants)"]
        IP["ip — instruction pointer"]
        WIN["cf_stack_top — base of this<br/>frame's stack window"]
    end

    FRAMES --> FRAME
    WIN -.points into.-> STACK
```

The dispatch loop (`vm.rs::run`) is the classic fetch–decode–execute cycle:

```mermaid
flowchart LR
    A["READ_BYTE:<br/>fetch opcode at ip,<br/>ip += 1"] --> B["decode<br/>u8 → OpCode"]
    B --> C{"match opcode"}
    C -->|"Constant"| D["push constant"]
    C -->|"Add / Less / ..."| E["pop 2, push result"]
    C -->|"Jump / Loop"| F["adjust ip by<br/>2-byte offset"]
    C -->|"Call"| G["push new CallFrame"]
    C -->|"Return"| H["collapse frame,<br/>push result"]
    D --> A
    E --> A
    F --> A
    G --> A
    H --> A
```

- **Local variables are stack slots.** A local declared in a function lives at
  `cf_stack_top + index`; `GetLocal`/`SetLocal` read/write that slot directly. Slot 0 of each
  frame is reserved for the function/closure object itself.
- **Globals** live in a hash table keyed by interned name.
- **Control flow** uses two-byte (big-endian) jump offsets patched in by the compiler.

---

## 5. Where we are — Crafting Interpreters progress

rlox tracks the `clox` half of the book (chapters 14–30). Current status:

```mermaid
flowchart TD
    C14["14 Chunks ✅"] --> C15["15 Virtual Machine ✅"]
    C15 --> C16["16 Scanning ✅"]
    C16 --> C17["17 Compiling Expressions ✅"]
    C17 --> C18["18 Types of Values ✅"]
    C18 --> C19["19 Strings ✅"]
    C19 --> C20["20 Hash Tables ✅"]
    C20 --> C21["21 Global Variables ✅"]
    C21 --> C22["22 Local Variables ✅"]
    C22 --> C23["23 Jumping Back and Forth ✅"]
    C23 --> C24["24 Calls and Functions ✅*"]
    C24 --> C25["25 Closures ⚠️ in progress"]
    C25 --> C26["26 Garbage Collection ❌"]
    C26 --> C27["27 Classes & Instances ❌"]
    C27 --> C28["28 Methods & Initializers ❌"]
    C28 --> C29["29 Superclasses ❌"]
    C29 --> C30["30 Optimization ❌"]

    classDef done fill:#1b5e20,stroke:#a5d6a7,color:#fff;
    classDef wip fill:#e65100,stroke:#ffcc80,color:#fff;
    classDef todo fill:#37474f,stroke:#b0bec5,color:#fff;
    class C14,C15,C16,C17,C18,C19,C20,C21,C22,C23,C24 done;
    class C25 wip;
    class C26,C27,C28,C29,C30 todo;
```

**Legend:** ✅ done · ⚠️ in progress · ❌ not started

### What works today
- Arithmetic, comparison, logical `and`/`or` (short-circuit), `!`/`-`.
- Booleans, numbers, `nil`, string literals and concatenation.
- `print`, expression statements, global and local variables with proper block scoping.
- `if`/`else`, `while`, `for`.
- Function declarations, calls, parameters, arity, `return`; first-class functions (assigning a
  function to a variable and calling through it).

### Chapter 24 caveat (`✅*`)
Native functions (e.g. clox's `clock()` / `defineNative`) were **skipped** — there is no
`Obj::Native`.

### Chapter 25 — where the work actually stops
The **compile side** of closures is largely written: the compiler resolves upvalues
(`recursive_resolve_up_value`), records them on the `CompilerContext`, and emits a `Closure`
instruction followed by `is_local`/`index` operand byte-pairs.

The **runtime side is not wired up**:
- `OpCode::Closure` in the VM reads only the function constant and **does not consume** the
  upvalue operand bytes the compiler emits, so the instruction pointer misaligns afterward.
- There are **no `GetUpValue`/`SetUpValue` match arms** in the dispatch loop — they fall into the
  catch-all `_` arm, which silently stops the VM.
- There is no runtime `ObjUpvalue`, no open/closed upvalue capture.

So a program that actually *captures* a variable in a closure will not run correctly yet.
Finishing this is the immediate next milestone.

---

## 6. Known bugs & rough edges

These are documented inline in the source with `// BUG:` / `// NOTE:` markers. Summary:

| Area | Issue |
|------|-------|
| `memory.rs` | `allocate::<String>()` allocates the size of the `String` struct (~24 bytes), not the text length; copying longer content overflows the heap (undefined behavior). |
| `vm.rs` | Closure upvalue operands not consumed; `Get/SetUpValue` unhandled; arity mismatch logs but doesn't abort; `runtime_error` only logs. |
| `compiler.rs` | Dead comparisons `arity >= 255` and `jump > u16::MAX` (u8/u16 can't exceed their max) — clippy `absurd_extreme_comparisons`; `resolve_local` clones locals to dodge a borrow. |
| `hash_map.rs` | Load-factor uses integer division so it effectively resizes only when full (`find_bucket` can loop forever on a full table); `insert` bumps `size` even on overwrite; inconsistent key matching (pointer vs content). |
| `common.rs` | `PartialEq` guarded by always-true `matches!(self, _other)`; value/string equality is pointer identity, so computed strings compare unequal; hand-written `Into` returns bogus defaults/panics (should be `TryFrom`). |
| `metrics.rs` | `static mut EVENTS` is unsound (`static_mut_refs`). |
| general | Hand-allocated string memory is never freed (leaks); no GC yet. |

---

## 7. Rust-specific notes (learning context)

This project intentionally translates clox's C data model literally, which surfaces the seams
between C-style manual memory and idiomatic Rust:

- **`Vec` length as the single source of truth.** Locals were originally a fixed 255-slot `Vec`
  plus a separate `local_count`; the two drifted and caused a scoping bug. Now the compiler uses
  `locals.len()` / `push` / `truncate` — the length *is* the count.
- **`&self` vs `&mut self`.** Take a mutable borrow only when you actually mutate. A `&mut self`
  accessor forces every caller — even read-only ones — into an exclusive borrow of the whole
  struct, which is what forced `.clone()` workarounds. The fix mirrors std's `get`/`get_mut`
  split.
- **Raw pointers vs owned types.** `FatPointer` + `memory.rs` reimplement C strings. The
  idiomatic Rust equivalent (`Rc<str>` / `String`) would remove the `unsafe`, the leaks, and the
  pointer-identity equality bug — but the manual version is kept as the learning exercise.

---

## 8. Running it

```bash
cargo build
./target/debug/rlox        # runs first.lox (hardcoded in main.rs)
```

Debug tracing is controlled by the flags in `debug.rs` (`INFO`, `DEBUG`, `PRINT_STACK`).
