# rlox — Adding a Feature End to End

A practical map of *which files and functions you touch, in what order*, when you add
something new to the language. Grounded in the actual rlox pipeline, with `file:function`
anchors you can jump to. (For the "why" of the architecture, see
[ARCHITECTURE.md](ARCHITECTURE.md); this doc is the "where do I edit" checklist.)

---

## The pipeline, in one line

```
source text
   │
   ▼   src/scanner.rs         — turn characters into Tokens
Scanner ──► Token stream
   │
   ▼   src/compiler.rs        — single-pass Pratt parser; emits bytecode as it parses
Compiler ──► Chunk (bytecode + constant pool + line info)   [src/chunk.rs, src/value.rs]
   │
   ▼   src/vm.rs              — the dispatch loop reads opcodes and executes them
VM ──► result + captured output
```

Everything you add flows **left to right**: a new surface syntax must be *scanned*, then
*compiled* into one or more opcodes, then *executed* by the VM. If any stage is missing its
piece, the feature breaks at that stage. Add the pieces in pipeline order and test after the
VM arm lands.

The three kinds of "new thing" have three different (overlapping) checklists. Find the one
that matches what you're adding.

---

## Recipe A — a new operator / expression

*Example: modulo `%`, or a bitwise operator, or a new unary form.*

Order of edits:

1. **Scanner — recognize the characters.** `src/scanner.rs`
   - Add a variant to `enum TokenType` (`scanner.rs:38`).
   - Emit it from `scan_token` (`scanner.rs:160`) — for a single/double-char operator, in
     the match on the current character; for a keyword-like word, in `identifier_type`
     (`scanner.rs:400`).

2. **Compiler — give the token a parse rule.** `src/compiler.rs`
   - Add/extend the arm in `parse_rule` (`compiler.rs:107`). A `ParseRule` says: what to do
     when the token appears in *prefix* position, in *infix* position, and at what
     `Precedence`. A binary operator like `%` goes in the infix slot with a precedence; a
     prefix operator goes in the prefix slot.
   - The rule points at a parse function — usually an existing one: `binary`
     (`compiler.rs:1463`) for infix operators, `unary` (`compiler.rs:1418`) for prefix. If the
     new operator fits their shape, you only add a `match` arm *inside* `binary`/`unary` that
     emits your new opcode. If it needs novel parsing, write a new parse fn and reference it
     from the rule.

3. **OpCode — define the instruction.** `src/common.rs`
   - Add a variant to `enum OpCode` (`common.rs:45`) with the **next unused discriminant
     number**. Never renumber existing ones — the numbers are the on-the-wire bytecode.
   - Document its operand shape in the doc comment (does it read bytes after itself? how
     many?).

4. **Compiler — emit it.** Back in the parse fn from step 2, call `emit_opcode`
   (`compiler.rs:1206`) / `emit_bytes` (`compiler.rs:1200`) to write your opcode (plus any
   operand bytes) into the chunk.

5. **VM — execute it.** `src/vm.rs`
   - Add a `Some(OpCode::Yours) => { ... }` arm to the dispatch loop in `run`
     (`vm.rs:331`). Model it on a sibling: `Add` (`vm.rs:369`) for a binary op, `Negate`
     (`vm.rs:357`) for a unary op.
   - If your opcode has operand bytes, **read them with the `READ_BYTE!` / `READ_CONSTANT!`
     macros** (see `Call` at `vm.rs:480`, `Constant` at `vm.rs:424`). Reading exactly the
     bytes you emitted is not optional — see the operand-symmetry gotcha below.

6. **Test.** Add an end-to-end case in the `vm.rs` test module: run source, assert output
   (see [VM_TESTING.md](VM_TESTING.md)).

---

## Recipe B — a new statement / keyword

*Example: a `switch`, a `do/while`, a new declaration form.*

1. **Scanner — the keyword.** `src/scanner.rs`
   - Add the `TokenType` (`scanner.rs:38`) and recognize the word in `identifier_type`
     (`scanner.rs:400`) — this is where reserved words are matched against identifiers.

2. **Compiler — statement dispatch.** `src/compiler.rs`
   - Hook it into `statement` (`compiler.rs:803`) — or `declaration` (`compiler.rs:407`) if it
     introduces a name (like `var`/`fun`). This is the top-level "what kind of statement is
     this?" switch.
   - Write the parse function for the construct. It consumes the tokens and emits the
     opcodes. For control flow you'll emit **jumps** — reuse the jump machinery
     (`JumpIfFalse`/`Jump`/`Loop` opcodes and the emit/patch helpers) rather than inventing
     new ones.

3. **OpCode / VM** — only if the construct needs a *new* instruction. Most statements are
   built from existing opcodes (jumps, pops, get/set). If you do need one, follow Recipe A
   steps 3 + 5.

4. **Test** — end-to-end.

---

## Recipe C — a new value / object type

*Example: an array, a map, a class instance — anything that lives at runtime as data.*

This one is different: the danger isn't a missing stage, it's the **exhaustive `match`
sites** scattered across the code. Adding a variant makes the compiler flag most of them —
which is the good case. The dangerous ones are the `match`es with a catch-all `_ =>` arm,
which compile fine but silently do the wrong thing.

1. **Decide the layer.** `src/common.rs`
   - Small, inline, copyable (a bool-like)? → new `Value` variant (`common.rs:131`).
   - Heap-allocated / variable-sized / shared (string, function, array)? → new `Obj` variant
     (`common.rs:357`). This is almost always the answer for a real object.

2. **Fix every match on the type.** After adding the variant, `cargo build` and let the
   compiler list the non-exhaustive matches. Known sites to check:
   - `format_value` (`vm.rs:823`) — how it prints. **Has a `_ =>` catch-all**, so it won't
     error; it'll print `Debug` junk unless you add an explicit arm.
   - `Obj` / `Value` `PartialEq` (`common.rs`, around `common.rs:408`) — equality semantics.
   - The `TryFrom`/`From` conversions for `Obj`/`Value` (`common.rs:445`+) — if your type is
     ever extracted from a `Value`.
   - `debug.rs:50` — the disassembler's value printer (debug output only).

3. **Constructing it.** If it's heap-allocated like a string, you allocate through the
   `memory` helpers and (for strings) intern via the string `table`. See the `concat` /
   `define_native` interning pattern in `vm.rs`.

4. **Opcodes to build/consume it** — if the language needs syntax to *create* or *operate on*
   the type, that's Recipe A/B on top of this.

5. **Test** — end-to-end, plus consider unit tests for the type's own methods.

---

## Master checklist

| Stage | File | Anchor | Needed for… |
|---|---|---|---|
| Token type | `scanner.rs` | `enum TokenType` (`:38`) | A, B |
| Tokenize | `scanner.rs` | `scan_token` (`:160`) / `identifier_type` (`:400`) | A, B |
| Parse rule | `compiler.rs` | `parse_rule` (`:107`) | A |
| Parse fn | `compiler.rs` | `binary`/`unary` (`:1463`/`:1418`) or new | A |
| Statement dispatch | `compiler.rs` | `statement` (`:803`) / `declaration` (`:407`) | B |
| OpCode variant | `common.rs` | `enum OpCode` (`:45`) | A, B (new instr) |
| Emit bytecode | `compiler.rs` | `emit_opcode`/`emit_bytes` (`:1206`/`:1200`) | A, B |
| Dispatch arm | `vm.rs` | `run` loop (`:331`) | A, B (new instr) |
| Read operands | `vm.rs` | `READ_BYTE!`/`READ_CONSTANT!` (see `Call` `:480`) | any opcode w/ operands |
| Value/Obj variant | `common.rs` | `enum Value` (`:131`) / `enum Obj` (`:357`) | C |
| Fix matches | `vm.rs`, `common.rs`, `debug.rs` | `format_value` (`:823`), `PartialEq` (`:408`), `TryFrom` (`:445`), disasm (`:50`) | C |
| Test | `vm.rs` | `#[cfg(test)] mod tests` | all |

---

## Gotchas (learned the hard way)

- **Operand symmetry — emit N bytes, consume N bytes.** Whatever operand bytes the compiler
  writes after an opcode, the VM's dispatch arm *must* read exactly that many, or `ip` drifts
  onto operand bytes and misreads them as opcodes. This is precisely the recursion/`Closure`
  bug: the compiler emitted upvalue bytes the `Closure` arm never consumed, derailing the VM
  (see the `Closure` arm note at `vm.rs:467` and PROGRESS.md).

- **Exhaustive matches are your friend; catch-alls are the trap.** A `match` with no `_ =>`
  arm *won't compile* until you handle your new variant — the compiler hands you the to-do
  list for free. A `match` with `_ =>` compiles silently and does the wrong thing. When
  adding a `Value`/`Obj` variant, grep for `_ =>` on those types specifically.

- **Never renumber `OpCode` discriminants.** The numbers (`Return = 1`, `Constant = 2`, …)
  *are* the bytecode. Add new opcodes at the end with the next free number; reordering
  silently reinterprets every existing chunk.

- **Strings must be interned.** Any new path that creates a string must go through the
  interning `table`, or pointer-identity equality breaks (see HASHMAP.md §4). This bit
  `concat` before it was fixed.

- **Add the pieces in pipeline order and test at the end.** A half-wired feature (token but
  no parse rule, or opcode emitted but no VM arm) fails in confusing ways. Get one vertical
  slice working end to end, then extend.
