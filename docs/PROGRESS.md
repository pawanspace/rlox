# rlox — Progress Report

A summary of what changed and what was learned during a focused hardening pass on the rlox bytecode
VM. The test suite went from *7 tests (1 failing)* to **22 passing, 0 failing**; build warnings
dropped from ~38 to 22 (all remaining are dead-code for intentionally unwired scaffolding). Six
design/reference docs were added and the whole codebase was documented.

---

## 1. What we accomplished

### Bugs fixed (correctness & memory safety)
- **Local scoping** — locals from an exited scope still resolved and shadowed globals. Root cause: a
  fixed 255-slot `Vec` plus a separate `local_count` that drifted. Switched to `Vec` length as the
  single source of truth (`push`/`truncate`/`len`).
- **Heap buffer overflow** — `allocate::<String>()` reserved the 24-byte `String` struct size, then
  copied the text into it. Replaced with a byte-oriented `allocate_bytes(len)` and a matching
  `drop_bytes` (align 1).
- **`static mut EVENTS` unsoundness** — replaced with `OnceLock<Mutex<HashMap<..>>>`; removed all
  `unsafe`.
- **Non-ASCII hashing panic** — the FNV hash looped over byte length but indexed a `char` vector.
  Now iterates `value.bytes()`.
- **`get` / `get_mut` panics** — both called `find_entry(..).unwrap()`, panicking on absent keys. Now
  match the `Option`.
- **`insert` size drift** — `size` was incremented even on overwrite. Now only for new keys;
  `delete` decrements it.
- **Load factor** — the resize threshold used integer division (`(size+1)/capacity`) that truncated
  to 0 until nearly full. Fixed by cross-multiplying `(size+1)*100 > capacity*load_factor`.
- **Hash-table tombstones** — `insert` stopped at the first tombstone (creating duplicates) and
  `delete` could miss keys past a tombstone. `insert` now uses a probe that remembers the first
  tombstone but scans on; `delete` uses the lookup probe.
- **Key-matching inconsistency** — three different "same key?" rules unified to pointer identity
  (`FatPointer::eq`), valid because every string is interned.
- **String concatenation & equality** — `concat` didn't intern its result, and string literals were
  stored *with their surrounding quotes*. Now `concat` interns, and literals are unquoted, so
  `"a" + "b" == "ab"` is `true`.
- **Arity / argument overflow** — parameter and argument counts (`u8`) could overflow on the 256th;
  a shared `increment_arity` helper errors and caps at 255. Arity mismatch now aborts the call.
- **Dead comparisons** — `arity >= 255`, `jump > u16::MAX`, `scope_depth <= 0`, `locals.len() <= 0`
  were all always-true/false. Fixed; `cargo clippy` reports no `absurd_extreme_comparisons`.
- **Lying `Into` conversions** — hand-written `Into<X> for &Value`/`Obj` returned `0.0`/`false`,
  panicked, or built a dangling pointer. Replaced with `TryFrom` returning `Result`.
- **`ValueArray::count()` underflow** — renamed to `last_index()` and guarded the empty case.
- **Recursion broken** — a function's self-reference (e.g. `fact` inside `fact`) went through the
  upvalue-resolution walk, which unconditionally recorded a phantom upvalue (index `-1 as u8 =
  255`). The compiler then emitted upvalue operand bytes that the half-implemented `Closure` opcode
  never consumed, so the ip drifted onto an invalid opcode and the VM halted. Fixed by only
  recording an upvalue when the recursive lookup actually finds one; a top-level recursive call now
  resolves as a global (Ch. 24) and runs (`fact(5)` → `120`).

### Refactors & features
- **Result-based runtime errors** — introduced `RuntimeError`; fallible ops return `Result`, the
  dispatch loop maps to `InterpretRuntimeError(RuntimeError)`, and `main` prints to stderr. Errors
  used to be silently swallowed (a bare `bool` + a logging `runtime_error`).
- **VM output capture** — `interpret` returns `(InterpretResult, Vec<String>)`; the `Print` opcode
  records output so tests can run Lox source and assert on it.
- **Dead-guard / accessor cleanups** — removed always-true `matches!(self, _other)` guards; added a
  shared `current_context()`/`current_context_mut()` split; inlined a needless `get_rule` wrapper.

### Tests
- From 7 (1 failing) to **22 passing**: memory round-trip + `drop_bytes`; hash-map delete,
  insert-overwrite, tombstone lookup/reinsert, `get_mut`, resize; hasher empty/non-ASCII; and
  end-to-end VM tests (arithmetic precedence, the scoping regression, concat equality, arity abort,
  recursion, first-class calls).

### Hygiene
- Removed stray `println!`s, unused imports, spurious `unsafe`, needless parens; dropped the unused
  `rustfix` dep; bumped `num-derive` 0.3→0.4; added `.gitignore` and stopped tracking `target`/`.idea`.

### Documentation
- Documented all 12 source files (module headers + item docs), and added:
  `ARCHITECTURE.md`, `HASHMAP.md`, `TEST_PLAN.md`, `VM_TESTING.md`, `ERROR_HANDLING.md`,
  `CONVERSIONS.md`.

---

## 2. What we learned

### Rust
- **A `Vec` and a length field are redundant** — one becomes a lie. Use `len()`/`push`/`truncate`.
- **`&self` vs `&mut self`** — take a mutable borrow only when you mutate. A `&mut self` *method*
  borrows the *whole* struct, so even reading another field through it conflicts. `let mut x` makes
  the *binding* rebindable, not the referent mutable.
- **`From`/`Into` and `TryFrom`/`TryInto`** — implement `From`/`TryFrom`; the `Into`/`TryInto` side
  comes free via blanket impls. `x.into()` literally calls `from`. The freebie is one-directional
  (coherence), so implementing `Into`/`TryInto` directly fails to compile.
- **`Result` + `?` beats returning `bool`** — a `bool` error signal is silent-by-default (you can
  forget to bail — that was the arity bug). `Result` is `#[must_use]` and `?` makes forgetting a
  compile error.
- **Interior mutability vs `static mut`** — a mutable global is unsound as `static mut`; use
  `OnceLock<Mutex<..>>` (init-once + runtime-exclusive access) with no `unsafe`.
- **Manual memory** — `Layout` carries size *and* alignment; `dealloc` must use the *same* layout as
  `alloc`. Byte buffers use alignment 1; `Layout::new::<T>()` gives a *type's* size, not a byte count.
- **Undefined behavior** — breaking the rules (out-of-bounds write, `static mut` aliasing) means the
  compiler guarantees nothing; it can *look* fine and betray you later. UB only lives in `unsafe`.
- **Miri** — an interpreter for Rust's MIR that detects UB deterministically; the real validator for
  `unsafe`/memory code (a round-trip test can't prove absence of an overflow).
- Smaller: deferred initialization, associated types (`type Error`), raw string literals (`r#"…"#`),
  captured format args (`{x}`), and integer pitfalls (truncating division, unsigned `<= 0`, `u8`
  overflow).

### VM / interpreter internals
- **Open addressing + linear probing**, and why deletion needs **tombstones** (and the correct
  insert probe: remember the first tombstone, scan to a match or `Vacant`).
- **Load factor** as the guardrail that keeps probe chains short and prevents a full-table hang.
- **String interning** — one canonical pointer per string makes pointer-identity equality valid; it
  must be airtight (literals *and* `concat`).
- **Error reporting belongs at the boundary** — the VM produces the error as data and propagates it;
  `main` decides where it goes. Same principle as capturing `print` output.
- **How interpreters are tested** — "run source, assert output" end-to-end (the Crafting
  Interpreters suite works this way), with isolated unit tests for leaf pieces (hash table, hasher).
  This is why the output-capture refactor was the high-leverage testing investment.

### Process
- **Tests surface latent bugs** — most bugs here weren't new; writing a test dragged them into the
  light (the concat test exposed the quote bug; the delete test exposed the `get` panic).
- **Keep comments in sync with code** — a recurring lesson: every fix left a `// BUG:` comment
  describing the old, now-wrong behavior. A stale comment is a lie.
- **Which layer did this teach me?** — for a project learning VMs + languages + Rust at once, it
  helps to name whether a bug was a Rust mistake, a VM-mechanics mistake, or a language-semantics
  mistake.

---

## 3. Where things stand

- **Chapters (Crafting Interpreters, clox):** 14–24 done — including **recursion**, which was found
  broken (a global self-reference registered a phantom upvalue, derailing the VM) and fixed; it
  needs no closures, matching Ch. 24. **25 (closures)** is compile-side only — the VM still doesn't
  consume upvalue operands or handle `Get/SetUpValue`, so closures that *capture outer locals* don't
  work yet; **26+ (GC, classes, inheritance, optimization)** not started.
- **Known remaining work** (see [tasks.md](../tasks.md)):
  - Deferred to chapters: finishing closures — upvalue *capture* (Ch. 25) — and the string-memory
    leak → GC (Ch. 26).
  - Open cleanups: `&mut self` → `&self` batch (clippy `needless_pass_by_ref_mut`), the
    `resolve_local` clone, re-enabling the REPL/CLI.
  - Structural refactors: `contexts` `Vec` + index arithmetic (#2), the read/write accessor split (#3).
  - Error-handling polish: reset the stack + attach line/stack-trace to `RuntimeError`; non-zero
    exit code in `run_file`.
- **Health:** builds clean, 22/22 tests pass, `cargo clippy` free of `absurd_extreme_comparisons`,
  22 dead-code warnings (intentional scaffolding).
