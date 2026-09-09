# rlox — Test Plan

Current state: **7 tests, all in `hash_map.rs`/`hasher.rs`; 1 is failing** (a real open bug).
Nothing tests the scanner, compiler, VM, string allocation, or metrics. This plan builds
coverage in layers, cheapest and highest-value first, and turns every bug in
[`tasks.md`](../tasks.md) into a regression test.

Legend: ✅ implemented · ⬜ todo · 🔴 currently failing (open bug) · ⏸ blocked on refactor.

---

## Layer 1 — Unit tests (pure, no I/O)

Direct tests of individual modules. Fast, no interpreter needed.

### memory
- ✅ **long-string round-trip** (`can_allocated_long_string`) — `allocate_bytes(src.len())` for a
  string **> 24 bytes**, copy the bytes in, `read_string` them back, assert byte-identical.
  Exercises the heap-overflow regime and asserts size-to-length.
- ⬜ `drop_bytes` on an `allocate_bytes` buffer does not crash (layout matches).

> **Important — validating the memory-safety fixes with Miri.** A functional round-trip test
> exercises the allocation path but **cannot prove** the absence of a heap overflow: an
> out-of-bounds write is undefined behavior and may silently "work" (returning the right bytes
> while corrupting adjacent memory), so the assertion can pass even on buggy code. To
> *deterministically* catch the overflow — and confirm the fix — run the tests under Miri, which
> models allocation bounds:
>
> ```
> cargo +nightly miri test
> ```
>
> Miri would flag the old `allocate::<String>()` out-of-bounds write as an error and pass on the
> fixed `allocate_bytes`. Treat Miri as the real validator for anything in `memory.rs` / `unsafe`;
> the plain `cargo test` round-trip is only a behavioral smoke test.

### hasher
- ✅ `can_calculate_hash` (existing).
- ⬜ empty string, and a **non-ASCII** string — the latter currently panics (`value.len()` bytes
  vs `chars[i]` indexing bug). Mark `#[ignore]` until that bug is fixed.

### hash_map
- ✅ insert/get/expand/reference (existing).
- 🔴 `can_hold_and_delete_multiple_keys` — **failing**, flags the delete/tombstone/probing bug.
  Keep as the regression test; fix the code, not the test.
- ⬜ insert-overwrite does not inflate `size` (open bug).
- ⬜ lookup succeeds after a probe chain crosses a tombstone (open bug).

### scanner
- ⬜ source string → expected `TokenType` sequence (operators, keywords vs identifiers, numbers,
  strings, two-char operators like `==`/`<=`, comments/whitespace skipped, EOF).
- ⬜ unterminated string → `Error` token.

### chunk / common / value
- ⬜ constant-index encoding: `write_constant` picks `Constant` (≤255) vs `ConstantLong` and
  `write_index` round-trips.
- ⬜ `Value`/`Obj` equality for numbers/bools/nil.
- ⬜ document (via a test) the known pointer-identity string-equality limitation.

---

## Layer 2 — Bug regression tests

One test per entry in `tasks.md`, named after the bug. Fixed → green; open → `#[ignore]` (or
allowed-red) with a comment linking the task, so the suite doubles as the tracker and a fixed bug
can't silently regress.

- ✅ (to add) heap overflow → covered by the memory long-string test above.
- ✅ (to add) `static mut` metrics → covered by the metrics test below.
- ⬜ local scoping (fixed) → needs Layer 3 (e2e) to assert program output.
- ⬜ open bugs (arity abort, dead comparisons, load factor, non-ASCII hash, etc.) → `#[ignore]`d
  tests that go green as each is fixed.

### metrics
- ⬜ `record("x", || 42)` returns `42` and does not panic; `display()` runs without panicking.
  (Smoke test that the `OnceLock<Mutex<..>>` path works and there's no `unsafe`/UB.)

---

## Layer 3 — End-to-end interpreter tests ⏸ (blocked on a refactor)

The highest-value layer: "run this Lox source, assert this output." This is what actually covers
the scoping bug, arithmetic, control flow, functions, and (later) closures.

**Blocker:** the VM writes output via `debug`/`println!` and returns only `InterpretResult`, so
there is nothing to assert on. Prerequisite refactor:

- ⬜ **Make VM output capturable** — have `print` (and errors) write to an injectable sink
  (e.g. `impl Write` or a `Vec<String>` on the VM) instead of `println!` directly. `interpret`
  returns/exposes the captured output.

Once unblocked:
- ⬜ arithmetic & precedence (`1 + 2 * 3` → `7`).
- ⬜ globals, locals, block scoping — including the **scoping regression**: the shadowed
  `for (var i = ...)` case that prints the global afterward.
- ⬜ `if`/`else`, `and`/`or` short-circuit, `while`, `for`.
- ⬜ functions: params, `return`, recursion, first-class assignment.
- ⬜ closures (once Chapter 25 runtime is finished).
- ⬜ runtime errors surface as errors (arity mismatch, type mismatch) rather than logging.

---

## Conventions

- Unit tests live in a `#[cfg(test)] mod tests` in each module.
- E2e tests go in a top-level `tests/` directory (integration tests) once Layer 3 is unblocked.
- Name regression tests after the bug (`fn overflow_long_string_roundtrip()`), and reference the
  `tasks.md` item in a comment.
- Keep `cargo test` green: an open-bug test is `#[ignore]`d (with a note) so a red suite always
  means a real regression, not a known-todo.
