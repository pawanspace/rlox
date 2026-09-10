# rlox — Test Plan

Current state: **15 tests passing, 1 `#[ignore]`d** (open reinsert-tombstone bug), 0 failing
(`hash_map.rs`, `hasher.rs`, `memory.rs`). Still nothing tests the scanner, compiler, VM behavior,
or metrics. This plan builds
coverage in layers, cheapest and highest-value first, and turns every bug in
[`tasks.md`](../tasks.md) into a regression test.

Legend: ✅ implemented · ⬜ todo · 🚫 written but `#[ignore]`d (open bug) · ⏸ blocked on refactor.

---

## Layer 1 — Unit tests (pure, no I/O)

Direct tests of individual modules. Fast, no interpreter needed.

### memory
- ✅ **long-string round-trip** (`can_allocated_long_string`) — `allocate_bytes(src.len())` for a
  string **> 24 bytes**, copy the bytes in, `read_string` them back, assert byte-identical.
  Exercises the heap-overflow regime and asserts size-to-length.
- ✅ `drop_bytes` on an `allocate_bytes` buffer does not crash (`can_drop_allocated_bytes`) —
  confirms the alloc/free layouts match (align 1). (Smoke test; Miri validates the layout pairing.)

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
- ✅ empty string and **non-ASCII** string — the non-ASCII panic (`value.len()` bytes vs `chars[i]`)
  is fixed; `hash` now iterates `value.bytes()`. Tests assert the empty-string seed value and that
  non-ASCII input hashes without panicking / deterministically.

### hash_map
- ✅ insert/get/expand/reference (existing).
- ✅ `can_hold_and_delete_multiple_keys` — now passing after the `get` `Option`-match fix (was
  failing on the `find_entry(..).unwrap()` panic).
  Keep as the regression test; fix the code, not the test.
- ✅ **insert-overwrite keeps size accurate** (`insert_overwrite_keeps_correct_size`). *Given* a
  table, `insert("k", 1)` then `insert("k", 2)`. *Expect* `map.size == 1` (one distinct key). Fixed:
  `insert` now increments `size` only for a new key.
- ✅ **lookup probes past a tombstone** (`get_probes_past_tombstone`). Keys `A`/`B` forced into the
  same bucket (equal `hash`, distinct `ptr`/`size`); `delete(A)` tombstones its slot; `get(B)` still
  returns `Some(&B)`. Confirms lookup skips tombstones (this was *not* actually a bug).
- ✅ **`get_mut` does not panic on a missing key** (`find_entry_mut_should_not_panic_for_missing_key`).
  `get_mut(missing).is_none()` and `get_mut(present).is_some()`.
- ✅ **resizes at the load factor** (`resizes_when_load_factor_exceeded`). Insert 8 distinct keys into
  `Table::init(10)` (80% > 70%); *expect* `capacity > 10`. Was red before the cross-multiply fix.
- 🚫 **re-insert after delete must not duplicate** (`reinsert_after_delete_does_not_duplicate`,
  `#[ignore]`d — open bug). `insert(A); insert(B)` (collide); `delete(A)`; `insert(B, new)`;
  *expect* `size == 1`. *Currently* `find_bucket` stops at the tombstone and writes a duplicate, so
  `size` becomes `2`. Un-ignore when HASHMAP.md gap #2 is fixed.

### scanner
- ⬜ **keyword vs identifier.** *Given* source `"var x"`, scan tokens. *Expect* `[Var, Identifier,
  Eof]` — `var` is a keyword, `x` is an identifier (the trie must not misclassify `x` or a word
  like `variable` as `Var`).
- ⬜ **operators and two-char operators.** *Given* `"a <= b == 1"`, scan. *Expect* `[Identifier,
  LessEqual, Identifier, EqualEqual, Number, Eof]` — `<=`/`==` are single tokens, not `< =`.
- ⬜ **number and string literals.** *Given* `"12.5 \"hi\""`, scan. *Expect* `[Number, String, Eof]`,
  with the `Number` token spanning `12.5` and the `String` token spanning the quoted text.
- ⬜ **whitespace/comments skipped.** *Given* `"a // note\n b"`, scan. *Expect* `[Identifier,
  Identifier, Eof]` (the `// note` comment and newline produce no tokens) and the second token's
  `line == 2`.
- ⬜ **unterminated string.** *Given* `"\"abc"` (no closing quote), scan. *Expect* an `Error` token.

### chunk / common / value
- ⬜ **short constant index.** *Given* a fresh chunk, `write_constant(v)` when the index is ≤255.
  *Expect* the emitted opcode byte is `OpCode::Constant` followed by a **1-byte** index, and the
  returned index reads back the same value from the constant pool.
- ⬜ **long constant index.** *Given* a chunk with >255 constants already, `write_constant(v)`.
  *Expect* the opcode is `OpCode::ConstantLong` followed by an **8-byte** index encoding.
- ⬜ **value equality.** *Expect* `Number(1.0) == Number(1.0)`, `Missing == Missing`,
  `Boolean(true) != Boolean(false)`, and `Number(1.0) != Boolean(true)` (cross-type is unequal).
- ⬜ **string equality is pointer identity (documents current behavior).** *Given* two `Obj::Str`
  built from separately-allocated buffers holding the same text, *expect* they compare **not
  equal** — pinning today's known limitation. Flip this assertion when the equality bug in
  `tasks.md` is fixed.

---

## Layer 2 — Bug regression tests

One test per entry in `tasks.md`, named after the bug. Fixed → green; open → `#[ignore]` (or
allowed-red) with a comment linking the task, so the suite doubles as the tracker and a fixed bug
can't silently regress.

- ✅ heap overflow → covered by `can_allocated_long_string` (memory).
- ✅ non-ASCII hash panic → covered by the hasher non-ASCII test.
- ✅ `get` panic on absent key → covered by `can_hold_and_delete_multiple_keys`.
- ⬜ `static mut` metrics → **not yet covered** (see the metrics test below).
- ⬜ local scoping (fixed in code) → needs Layer 3 (e2e) to assert program output; the concrete
  case is under Layer 3.
- ⬜ remaining open bugs (reinsert-past-tombstone duplicate, arity abort, dead comparisons)
  → one `#[ignore]`d test each, flipped to green as the bug is fixed.

### metrics
- ⬜ **record returns value and stores timing.** *Given* `record("x", || 2 + 3)`, *expect* the
  return value is `5` and, afterward, the events table contains the key `"x"`; then `display()`
  runs without panicking. Confirms the `OnceLock<Mutex<..>>` path works with no `unsafe`/UB.

---

## Layer 3 — End-to-end interpreter tests ⏸ (blocked on a refactor)

The highest-value layer: "run this Lox source, assert this output." This is what actually covers
the scoping bug, arithmetic, control flow, functions, and (later) closures.

**Blocker:** the VM writes output via `debug`/`println!` and returns only `InterpretResult`, so
there is nothing to assert on. Prerequisite refactor:

- ⬜ **Make VM output capturable** — have `print` (and errors) write to an injectable sink
  (e.g. `impl Write` or a `Vec<String>` on the VM) instead of `println!` directly. `interpret`
  returns/exposes the captured output.

Once unblocked (each: run the source, assert the captured output):
- ⬜ **arithmetic & precedence.** `print 1 + 2 * 3;` → `7` (not `9`).
- ⬜ **globals & assignment.** `var a = 1; a = a + 4; print a;` → `5`.
- ⬜ **block scoping.** `var x = 1; { var x = 2; print x; } print x;` → `2` then `1`.
- ⬜ **scoping regression** (guards the fixed bug). `var i = 10; while (i < 15) { i = i + 1; }
  for (var i = 8; i < 10; i = i + 1) {} print i;` → `15` (the `for`'s local `i` must not leak;
  before the fix this printed `10`).
- ⬜ **if/else.** `if (false) print "a"; else print "b";` → `b`.
- ⬜ **short-circuit.** `print false and (1/0);` → `false` with no divide evaluated;
  `print true or (1/0);` → `true`.
- ⬜ **while / for counts.** loop bodies run the expected number of times (e.g. `for (var i=0;
  i<3; i=i+1) print i;` → `0`,`1`,`2`).
- ⬜ **functions.** params + `return` (`fun add(a,b){ return a+b; } print add(2,3);` → `5`),
  recursion (factorial → correct value), and first-class assignment (`var f = add; print f(1,1);`
  → `2`).
- ⬜ **closures** (once Chapter 25 runtime is finished): a counter closure captures and mutates an
  upvalue across calls → `1`, `2`, `3`.
- ⬜ **runtime errors** surface as an error result (not a silent log): calling with the wrong arg
  count, or `-"x"`, yields `InterpretRuntimeError` and no bogus output.

---

## Conventions

- Unit tests live in a `#[cfg(test)] mod tests` in each module.
- E2e tests go in a top-level `tests/` directory (integration tests) once Layer 3 is unblocked.
- Name regression tests after the bug (`fn overflow_long_string_roundtrip()`), and reference the
  `tasks.md` item in a comment.
- Keep `cargo test` green: an open-bug test is `#[ignore]`d (with a note) so a red suite always
  means a real regression, not a known-todo.
