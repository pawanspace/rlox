# rlox — Runtime Error Handling: from `bool` to `Result`

How the VM signals runtime errors, why the old `bool`-based approach was bug-prone, and the
`Result`-based design that replaced it.

> **Status: implemented.** Fallible ops return `Result<(), RuntimeError>`; the dispatch loop maps a
> returned `Err` to `InterpretResult::InterpretRuntimeError(RuntimeError)`; and `main` prints the
> message to stderr. `runtime_error` now *constructs* the error (rather than logging). Sections 1–2
> below describe the old `bool` approach for context; §3–5 are the current design. Still to do:
> reset the stack + attach a line/stack trace to `RuntimeError`, and a non-zero exit code in
> `run_file`.

---

## 1. How it works today (`bool` + manual checks)

Operations that can fail return a `bool`, and `runtime_error` just logs:

```rust
fn runtime_error(&self, message: &str) {
    debug::info(format!("Runtime error: {:?}", message));   // logs only
}

fn execute_function(&mut self, ...) -> bool {
    if function.arity != arg_count {
        self.runtime_error("...");
        return false;            // caller must notice this and bail
    }
    // ...
    true
}
```

and the dispatch loop turns a `false` into a result:

```rust
Some(OpCode::Call) => {
    if !self.execute_function(...) {
        return InterpretResult::InterpretRuntimeError;   // manual bail
    }
    // ...
}
```

So there are **two separate jobs**, both done by hand:
- **report** the error (`runtime_error` — currently only logs; it should also reset the stack), and
- **unwind** — every caller must check the `bool` and `return` the error itself.

Rust has no non-local return (short of `panic!`, which is wrong for an ordinary language error), so
the unwind *must* be propagated up as a return value. That part is unavoidable; the problem is how
it's encoded.

---

## 2. Why this is bug-prone

The bug we hit with arity checking came straight from this pattern: `execute_function` called
`runtime_error(...)` (logged the message) but **fell through** and built the call frame anyway —
"I reported an error" and "I actually stopped" drifted apart, and nothing forced them together.

The `bool` pattern is silent-by-default:
- The signal is a bare `bool` whose meaning (`true` = success? keep going?) isn't self-evident and
  is easy to invert.
- Each caller must *remember* to check it and manually `return`. Miss one and the error is silently
  swallowed — execution continues in a corrupt state (e.g. a misaligned stack).
- `runtime_error` returning `()` carries no signal at all, so reporting and bailing are unlinked.
- **The compiler can't help** — ignoring a returned `bool` is perfectly legal, so a missing bail is
  not a compile error.

---

## 3. The idiomatic fix: `Result<(), RuntimeError>` + `?`

Encode "can fail" in the type system and let `?` do the propagation.

```rust
/// A runtime error, carrying whatever the reporter needs (message, line, ...).
struct RuntimeError { message: String /* , line, stack trace, ... */ }

impl VM {
    /// Report the error (print + reset the stack) AND produce the Err value,
    /// so call sites can `return Err(self.runtime_error(...))` or use `?`.
    fn runtime_error(&mut self, message: &str) -> RuntimeError {
        // print message + stack trace here, and self.reset_stack();
        RuntimeError { message: message.to_string() }
    }

    fn execute_function(&mut self, ...) -> Result<(), RuntimeError> {
        if function.arity != arg_count {
            return Err(self.runtime_error("wrong arg count"));
        }
        self.create_call_frame(function, arg_count);
        Ok(())
    }

    fn run(&mut self) -> InterpretResult {
        loop {
            let step: Result<(), RuntimeError> = /* match opcode, using `?` inside helpers */;
            if let Err(_e) = step {
                return InterpretResult::InterpretRuntimeError;
            }
        }
    }
}
```

Inside the loop, a fallible op is just `self.execute_function(...)?;` — the `?` returns the `Err`
automatically.

### Why this removes the footgun
- `Result` is `#[must_use]`: **ignoring it warns**, so a dropped error is visible.
- `?` propagates automatically — you *cannot* forget to bail; not handling the `Result` is a
  **compile error** (type mismatch), not a silent continue.
- Reporting and unwinding become one expression (`return Err(self.runtime_error(...))`), so they
  can't drift apart the way the arity bug did.
- The error is a real type that can grow (line number, call-stack trace) without touching every
  signature.

Net: a whole class of "logged but didn't stop" bugs turns from *silent runtime corruption* into
*compile errors*.

---

## 4. Migration sketch (when you take it on)

This touches `run` and every op that can fail, so it's a chapter-sized change — best done as its own
pass, not mid-feature.

1. Define `RuntimeError` (start with just a message; add line/trace later).
2. Change `runtime_error` to **return** `RuntimeError` and to reset the stack (closes the separate
   "`runtime_error` only logs" task).
3. Convert fallible helpers (`execute_function`, the binary/negate type checks, global lookups,
   `concat`, …) from `-> bool` / early `return` to `-> Result<(), RuntimeError>`.
4. In `run`, call them with `?` inside a helper that returns `Result`, and map a returned `Err` to
   `InterpretResult::InterpretRuntimeError` at the top.
5. Delete the manual `if !... { return ... }` checks as each is replaced by `?`.

Until then, the `bool` approach is correct where it's actually checked (e.g. the arity fix) — it's
just easy to get wrong, which is the reason to migrate.

---

## 5. Call-site inventory (`vm.rs`)

There are 9 `runtime_error` call sites to convert — the full surface of the refactor:

| Location | Op / method | Error |
|---|---|---|
| `BINARY_OP!` macro | `+ - * / < >` | "Expected two numbers" |
| `Negate` | unary `-` | "Expected number" |
| `Add` | string `+` | "Expected String on right side" |
| `Add` | `_` arm | "Unknown type" |
| `set_global_variable` | `SetGlobalVariable` | undefined variable on set |
| `push_obj_value_to_stack` | `GetGlobalVariable` | undefined variable |
| `execute_function` (Fun arm) | `Call` | arity mismatch |
| `execute_function` (Closure arm) | `Call` | arity mismatch |
| `execute_function` | `Call` | "Can only execute function" (non-callable) |

Two things to watch while converting:

1. **The `BINARY_OP!` macro is special.** It's textually pasted *into* the `run` loop, so it already
   does `runtime_error(...); return InterpretResult::InterpretRuntimeError;` directly rather than
   returning a `bool` to a caller. Decide whether the binary op becomes a helper that returns
   `Result` (cleaner, uniform with the rest) or the macro keeps returning the result value. Either
   works; it's just the one site that isn't already in a helper.

2. **The existing returns are inconsistent — audit each site.** Most sites `return
   InterpretResult::InterpretRuntimeError`, but the `Add` `_` ("unknown type") arm reports the error
   and then returns `InterpretResult::InterpretOk` — i.e. it logs a failure but tells the caller
   everything's fine. The `Result` migration is the moment to make every site actually bail with an
   error instead of silently continuing. (Done.)

---

## 6. Remaining polish (implementation notes)

The data for all of these already exists; the notes below are how to wire each up.

### Stack reset — easy
On a runtime error the value stack and call stack are left mid-computation. Clear them so a REPL
session (or anything that keeps the `VM` alive) starts clean: in `runtime_error`, before returning
the `RuntimeError`, set `stack_top = 0` (there's already a `reset_stack()`) and `frame_count = 0`.

### Non-zero exit code — easy
`run_file` currently prints the message but exits `0`. clox exits **70** on a runtime error so
scripts/CI can detect failure: after the `eprintln!(err.message)`, call `std::process::exit(70)`.
Do **not** do this in the REPL `prompt` — the REPL should report the error and keep looping.

### Line number — data exists, needs plumbing
Each `Chunk` has a parallel `lines: Vec<u32>` indexed by bytecode offset, and a `CallFrame` carries
its `function.chunk` + `ip`, so the line of the failing instruction is:
```rust
let line = current_frame.function.chunk.lines[current_frame.ip - 1]; // -1: ip already advanced
```
The snag: `runtime_error` takes `&self` and has **no access to `current_frame`** (it's a local in
`run`, not a field on `self`). To attach a line, pick one:
- pass `current_frame` (or just its `ip` + `&chunk`) into `runtime_error`, or
- have each call site compute the line and set it on the error, or
- store the current frame on `self` instead of as a `run` local (biggest ripple).

Then widen the type: `struct RuntimeError { message: String, line: u32 }`.

### Call-stack trace — same signal, more of it
For a full "at line X in function Y, called from …" trace, walk the active frames
`self.call_frames[0..frame_count]` and, for each, read `chunk.lines[ip - 1]` and `function.name`.
Same data as the single line, just iterated over the frame stack; add it onto the same
`runtime_error`/`RuntimeError` change above.

**Suggested order:** stack reset and exit code first (a few lines each, no signature changes), then
the line number (requires the `current_frame` plumbing), then the trace (extends the line work).
