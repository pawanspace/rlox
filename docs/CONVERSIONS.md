# rlox — `From` / `Into` / `TryFrom` / `TryInto`

A short reference on how Rust's conversion traits relate, written down because it came up while
replacing the lying `Into` impls with `TryFrom` (see [ERROR_HANDLING.md](ERROR_HANDLING.md) and the
`common.rs` conversions).

---

## The two pairs

| Infallible | Fallible | Return type |
|---|---|---|
| `From<T> for U` | `TryFrom<T> for U` | `U` / `Result<U, Error>` |
| `Into<U> for T` | `TryInto<U> for T` | `U` / `Result<U, Error>` |

`From<T> for U` reads "build a **`U`** from a **`T`**." `Into<U> for T` reads "turn a **`T`** into a
**`U`**." They describe the *same* conversion from opposite ends.

## You implement `From`/`TryFrom`; `Into`/`TryInto` come free

`Into`/`TryInto` are **not** implemented by hand. std defines them in terms of `From`/`TryFrom` with
blanket impls:

```rust
// in std
impl<T, U> Into<U> for T
where U: From<T>
{
    fn into(self) -> U { U::from(self) }              // just forwards to from()
}

impl<T, U> TryInto<U> for T
where U: TryFrom<T>
{
    type Error = U::Error;                            // reuses TryFrom's error type
    fn try_into(self) -> Result<U, U::Error> { U::try_from(self) }
}
```

So the moment you write `impl From<Obj> for Function`, the blanket fires and `Obj: Into<Function>`
exists automatically. Same for `TryFrom` → `TryInto`.

## `into()` and `from()` return the *same value*

Because `into`'s entire body is `U::from(self)`, these are two spellings of one conversion:

```rust
let v = Value::from(b);   // name the target explicitly, source is the argument
let v: Value = b.into();  // target inferred from context; calls the same from()
```

They run identical code and produce the identical value. The only difference is ergonomics:
- `T::from(x)` / `X::try_from(x)` — you name the **target** explicitly.
- `x.into()` / `x.try_into()` — the target is **inferred** from context (a `let` annotation, a
  function parameter type, etc.).

Same for the fallible pair: `X::try_from(v)` and `v.try_into()` both call your one `try_from` and
return the same `Result`.

## Why the freebie goes only one direction

`From → Into` is automatic, but `Into → From` is **not** (and `TryInto → TryFrom` isn't either).
There is no reverse blanket impl, deliberately:

1. **Coherence.** A `From → Into` blanket *and* an `Into → From` blanket would be mutually
   recursive / overlapping impls, which Rust's coherence rules forbid. std had to pick one direction
   and derive the other.
2. **`From` is the better primitive.** The `?` operator converts errors via `From::from`, and `From`
   is more ergonomic in generic bounds — so the ecosystem standardized on implementing `From`.

## The rule

> **Always implement `From` / `TryFrom`; never implement `Into` / `TryInto`.**

Implementing `Into`/`TryInto` directly collides with the blanket impls and fails to compile (e.g.
`the trait bound Function: From<Obj> is not satisfied`, or a duplicate-impl error). Implement the
`From`/`TryFrom` side and the `Into`/`TryInto` side appears for free, callable at every site.

Mnemonic: **`From` is the source of truth; `Into` is its free mirror.**

## In rlox

- **Infallible, into a `Value`:** `From<bool>`, `From<f64>`, `From<Obj>` for `Value` — every input
  maps to a variant, nothing fails.
- **Fallible, out of a `Value`/`Obj`:** `TryFrom<&Value>` for `bool`/`f64`/`Obj`/`FatPointer`, and
  `TryFrom<Obj>` for `FatPointer`/`Function` — a value might be the wrong variant, so these return
  `Result`. Call sites use `X::try_from(v)` and `.unwrap()` where the type is already guaranteed by
  a preceding check.
