# rlox — Runtime Error Handling: `bool` today, `Result` tomorrow

How the VM signals runtime errors, why the current approach is bug-prone, and the idiomatic
Rust refactor to `Result` + `?`. This is a design note / proposed refactor, not yet implemented.

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
