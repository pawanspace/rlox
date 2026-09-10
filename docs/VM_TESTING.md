# rlox — Testing VM / Language Behavior (Output Capture)

The primary way to test rlox's *language behavior* (arithmetic, scoping, functions, string ops,
closures, runtime errors) is to **run Lox source and assert on what it printed**. This document
explains why, and exactly how to wire it up.

For the layered plan and the full list of behavioral cases, see [`TEST_PLAN.md`](TEST_PLAN.md).

---

## 1. Why this approach

Language behavior is **emergent** — `1 + 2 * 3 == 7` isn't a property of any single function; it
comes out of scanner → compiler → VM working together. Testing one opcode or one method in isolation
means hand-building artificial state (valid bytecode, a stack layout) that only the real pipeline
produces correctly — brittle, and it doesn't test the thing you actually care about. So we test the
whole pipeline the way it's really used: feed a program, check its output.

This is the standard way interpreters/compilers are tested — including *Crafting Interpreters*'
own Lox test suite, which is `.lox` programs annotated with their expected output.

(Leaf components with clean contracts — the hash table, the hasher, `memory` — still get ordinary
isolated unit tests. This document is specifically about the *language behavior* layer.)

---

## 2. The blocker

Today the VM prints directly:
```rust
Some(OpCode::Print) => {
    debug::print_value(self.pop().as_ref().unwrap(), true);  // -> debug -> println!
}
```
`println!` writes to the process's stdout (file descriptor 1), which a test **cannot read back**
without OS-level tricks. And `interpret` returns only an `InterpretResult`, so there's nothing to
assert on. To make behavior testable we must route printed output through **state we own** instead
of straight to the terminal.

---

## 3. Implementation

### Step 1 — give the VM an output buffer
```rust
pub(crate) struct VM {
    // ...existing fields...
    output: Vec<String>,   // one entry per `print`
}
// in VM::init:
output: Vec::new(),
```

### Step 2 — format a `Value` into its Lox display string
```rust
fn format_value(&self, v: &Value) -> String {
    match v {
        Value::Number(n)        => n.to_string(),
        Value::Boolean(b)       => b.to_string(),
        Value::Missing          => "nil".to_string(),
        Value::Obj(Obj::Str(p)) => memory::read_string(p.ptr, p.size),
        Value::Obj(o)           => format!("{:?}", o),
    }
}
```

### Step 3 — make the `Print` opcode capture (and still print for real runs)
```rust
Some(OpCode::Print) => {
    let v = self.pop().as_ref().unwrap().clone();
    let s = self.format_value(&v);
    println!("{s}");        // keep normal runs visible
    self.output.push(s);    // and record it for tests
}
```

### Step 4 — let callers read the captured output
Either expose the field to in-module tests (they can already see private fields), or — the more
idiomatic shape — have `interpret` **return** the captured lines and let `main` do the printing
("return data, don't print it"):
```rust
pub(crate) fn interpret(&mut self, source: String) -> (InterpretResult, Vec<String>) {
    // ...compile + run as today...
    (result, self.output.clone())
}
```

### Step 5 — a test helper
```rust
#[cfg(test)]
mod tests {
    use super::*;
    fn run(src: &str) -> Vec<String> {
        let mut vm = VM::init();
        vm.interpret(src.to_string());
        vm.output.clone()
    }

    #[test] fn arithmetic_precedence() {
        assert_eq!(run("print 1 + 2 * 3;"), ["7"]);
    }

    // Guards the fixed local-scoping bug end-to-end.
    #[test] fn scoping_regression() {
        let src = "var i = 10; while (i < 15) { i = i + 1; } \
                   for (var i = 8; i < 10; i = i + 1) {} print i;";
        assert_eq!(run(src), ["15"]);
    }

    // The concat / string-equality bug: red today, green once concat interns.
    #[test] fn concat_string_equality() {
        assert_eq!(run(r#"print "a" + "b" == "ab";"#), ["true"]);
    }
}
```

---

## 4. How it tests `concat` (worked example)

`run(r#"print "a" + "b" == "ab";"#)` executes the real pipeline:
1. bytecode: push `"a"`, push `"b"`, **Add** (string `+` → `concat`), push literal `"ab"`, **Equal**, **Print**;
2. `concat` builds `"ab"` as a fresh, **un-interned** `FatPointer` (different pointer than the literal);
3. `Equal` compares `Value`s by **pointer** → different → `false`;
4. `Print` pushes `"false"` to `output`;
5. the test asserts `["true"]` → **fails** — red for exactly the right reason.

Fix `concat` to intern its result (reuse the existing pointer for `"ab"`), and the same pointer makes
`Equal` return `true`, turning the test green. No private methods or hand-built stack needed.

---

## 5. Variants (and which to pick)

| Shape | What | Pick? |
|---|---|---|
| `Vec<String>` field on the VM | Push each printed line into an owned buffer | Simplest; good enough for a learning VM. **Recommended start.** |
| `interpret` **returns** the output; `main` prints | Core produces data, outer layer prints | Most idiomatic ("return, don't print"); do this if you want the cleanest design. |
| Inject `&mut impl Write` | VM writes formatted bytes to a sink; prod passes `stdout`, tests pass `Vec<u8>` | Most flexible/general; more machinery than needed here. |
| Snapshot tests (`insta` crate) | Auto-record & diff expected output | Add later if the suite grows large. |

**Recommendation:** start with the `Vec<String>` field (Steps 1–5). If you want it more idiomatic,
promote it to "`interpret` returns the lines, `main` prints them." Reach for `impl Write` or `insta`
only if a concrete need appears.

---

## 6. Caveats

- **Number formatting:** `f64::to_string()` gives `"7"` for `7.0` and `"2.5"` for `2.5` — close to
  clox's `%g` but not identical for every value. Adjust `format_value` if you want exact parity.
- **Errors:** runtime/compile errors currently go through `debug`/`runtime_error` (logging only). To
  assert on error behavior, route those through the same capture (or assert on the returned
  `InterpretResult`).
- **Streaming vs buffered:** pushing to a buffer changes *when* output is materialized. Keeping the
  `println!` in Step 3 preserves live streaming for real runs while still capturing for tests.
