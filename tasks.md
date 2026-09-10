# rlox — Bug & Task Tracker

Prioritized list of known bugs and incomplete features. Each is documented inline in the source
with `// BUG:` / `// NOTE:` markers. Ordered roughly most-important first. Check off as fixed.

Priority key: 🔴 critical (memory safety / crashes / wrong results) · 🟠 correctness ·
🟡 cleanup / hygiene.

---

## 🔴 Critical

- [x] **Heap buffer overflow in string allocation** — fixed. Replaced `allocate::<String>()`
  with a byte-oriented `memory::allocate_bytes(len)` (`Layout::from_size_align(len, 1)`) and
  updated all three call sites (`compiler.rs::create_new_string`, `common.rs` `From<&mut str>`,
  `vm.rs::concat`). Added a matching `drop_bytes(ptr, len)` (align 1) for the future free path.

- [x] **`static mut EVENTS` is unsound** — fixed. Now `static EVENTS: OnceLock<Mutex<HashMap<..>>>`
  with `get_or_init` + `lock`; removed `static mut`, `init_events`, and all `unsafe`.

- [ ] **Allocated string memory is never freed** — `memory.rs`. Every interned/concatenated
  string leaks. Acceptable for now; real fix is Chapter 26 (GC) or switching to owned types.

---

## 🟠 Correctness

- [ ] **Closures don't run (Chapter 25 incomplete)** — `vm.rs`.
  - `OpCode::Closure` reads only the function constant and does not consume the `is_local`/`index`
    upvalue operand bytes the compiler emits → instruction pointer misaligns.
  - No `GetUpValue` / `SetUpValue` match arms → they hit the catch-all `_` and silently halt the VM.
  - No runtime `ObjUpvalue`, no open/closed capture.
  *Fix:* implement the runtime upvalue model (consume operands in `Closure`, add the two opcodes,
  capture from the enclosing frame's stack).

- [ ] **Arity mismatch doesn't abort** — `vm.rs::execute_function`. On wrong argument count it
  calls `runtime_error` (which only logs) then proceeds to build the call frame anyway.
  *Fix:* return an error / stop execution.

- [ ] **`runtime_error` only logs** — `vm.rs`. It prints via `debug::info` but doesn't reset the
  stack or surface a real error to the caller. *Fix:* proper error propagation + stack trace.

- [ ] **Computed-string equality is pointer identity** — `common.rs` (`Value`/`Obj`/`FatPointer`
  `PartialEq`). Literals are interned so they share a pointer, but `concat` allocates fresh,
  uninterned memory, so `"a" + "b" == "ab"` is `false`. *Fix:* intern in `concat`, or compare by
  content/hash.

- [x] **`get_mut` panics on absent keys** — fixed. `get_mut` now matches the `Option` from
  `find_entry_mut` instead of `.unwrap()`, so a missing key returns `None`. Covered by
  `find_entry_mut_should_not_panic_for_missing_key`.

- [x] **Hash table load factor** — fixed. `ensure_capacity` now cross-multiplies
  (`(size+1)*100 > capacity*load_factor`) instead of the integer-division form that truncated to 0
  and only resized near-full. Resizes at the 70% target. Covered by `resizes_when_load_factor_exceeded`.
  (`delete` also now decrements `size`, so the count reflects live entries.)

- [x] **`insert` inflates `size` on overwrite** — fixed. `insert` now increments `size` only for a
  genuinely new key (`if !new_value`), so overwrites no longer drift the count. Covered by
  `insert_overwrite_keeps_correct_size`.

- [ ] **Inconsistent key matching** — `hash_map.rs`. `is_occupied` matches by pointer (`memory::eq`)
  while `find_entry_index` uses `FatPointer::eq` and `find_entry_with_value` uses string content;
  `is_occupied` also treats a tombstone as free, so a duplicate key can be inserted ahead of an
  existing one. *Fix:* one consistent match rule; probe past tombstones when checking existence.

- [ ] **Dead comparison: `arity >= 255`** — `compiler.rs::function`. `arity` is `u8` (max 255), and
  it's incremented *before* the check, so the guard can't work and a 256th parameter overflows.
  *Fix:* check `arity == 255` before incrementing (clox style).

- [ ] **Dead comparison: `jump > u16::MAX`** — `compiler.rs::emit_loop` / `patch_jump`. `jump` is
  `u16`, so the overflow guard is always false; long jumps silently wrap. *Fix:* compute the
  distance in `usize` and compare against `u16::MAX` before casting.

- [ ] **`Into` impls that lie** — `common.rs`. `Into<f64>`/`Into<bool>` return `0.0`/`false` on the
  wrong variant; `Into<Obj>`/`Into<FatPointer>` panic or return a dangling pointer to a temporary
  `"".to_string()`. *Fix:* use `TryFrom` returning `Result` for fallible conversions.

- [ ] **`ValueArray::count()` underflows on empty** — `value.rs`. `len() - 1` panics if empty; the
  name is misleading (it returns an index). *Fix:* rename / guard.

- [x] **Non-ASCII hashing panic** — fixed. `hash` now iterates `value.bytes()` (canonical FNV-1a),
  so multi-byte characters no longer cause an out-of-bounds `chars[i]` panic; ASCII hashes
  unchanged. Covered by empty-string and non-ASCII tests in `hasher.rs`.

---

## 🟡 Cleanup / hygiene

- [ ] **REPL/CLI disabled** — `main.rs` hardcodes `first.lox` and sets `RUST_BACKTRACE` in code;
  the CLI arg and REPL are commented out. Re-enable once stable.
- [ ] **`always-true `matches!(self, _other)` guards** — `common.rs` `PartialEq` impls; `_other` is
  a catch-all binding, so the guard is dead code. *Fix:* drop it, match directly.
- [ ] **Stray debug output** — e.g. `println!("Entry index …")` in `hash_map.rs`, `println!` in
  `chunk.rs::get_offset`, per-frame prints in `vm.rs`. Route through `debug` flags, default off.
- [ ] **Remaining `&mut self` → `&self`** — clippy `needless_pass_by_ref_mut` flags
  `str_to_float`, `prev_token_to_string`, `get_existing_string`, `resolve_from_locals` (compiler),
  `return_op`/`print_debug_info` params (vm), `identifier_type` (scanner), `get_at_index` (hash_map).
- [ ] **`resolve_local` clone** — `compiler.rs` clones `locals` to dodge a borrow because
  `resolve_from_locals` takes `&mut self`; making that method `&self` removes the clone.
- [ ] **`rustfix` dependency** — `Cargo.toml` lists a build tool as a runtime dep; likely mistaken.
- [ ] **Bump `num-derive` 0.3 → 0.4** to clear the non-local-impl warning.
- [ ] **`.gitignore`** — add `target/` and `.idea/`.
- [ ] **94 build warnings** — mostly unused imports/vars; clean up so real warnings stand out.

---

## Structural refactors (from the code review)

- [x] **#1 Locals: `Vec` length as source of truth** (removed `local_count`; fixed scoping bug).
- [ ] **#2 `contexts` Vec + `current_context` index arithmetic** — replace fragile `+1`/`-1`
  indexing (`function`, `recursive_resolve_up_value`) with push/pop + `last_mut()` access.
- [ ] **#3 Read/write accessor split** — finish converting read-only methods to `&self`
  (see clippy list above); mirrors std `get`/`get_mut`.
