//! # The constant pool (`ValueArray`)
//!
//! Bytecode instructions are just bytes, so a literal like `3.14` or `"hi"`
//! cannot be stored *inside* the instruction stream directly. Instead each
//! `Chunk` keeps a side table of literal `Value`s — the *constant pool* — and
//! the `Constant`/`ConstantLong` instructions carry a small integer *index*
//! into this pool. At runtime the VM reads the index and looks the value up
//! here. This is the standard bytecode technique (clox calls it `ValueArray`)
//! and it keeps instructions compact: a 1-byte index instead of an inline
//! 8-byte float.
use crate::common;

/// A growable list of constant `Value`s for one `Chunk`.
#[derive(Debug, Clone)]
pub(crate) struct ValueArray {
    /// The constants, addressed by their position (index) in this vector.
    pub values: Vec<common::Value>,
}

impl ValueArray {
    /// Create an empty constant pool.
    pub(crate) fn init() -> ValueArray {
        ValueArray { values: vec![] }
    }

    /// Append a constant and (implicitly) hand back its index via `count`.
    pub(crate) fn append(&mut self, value: common::Value) {
        self.values.push(value);
    }

    /// Look up a constant by index. Clones because `Value` is returned by value.
    pub(crate) fn get(&self, index: usize) -> common::Value {
        // TODO: `unwrap` panics on an out-of-range index; a real implementation
        // would return an error or `Option` instead of trusting the bytecode.
        (*self.values.get(index).unwrap()).clone()
    }

    /// The index of the most-recently-appended constant (i.e. `len() - 1`).
    ///
    /// This is used right after `append` to get the new constant's index.
    // BUG: `len() - 1` underflows (panics in debug / wraps to a huge number in
    // release) when the pool is empty. It is only safe because callers always
    // call it immediately after `append`. The name is also misleading — it
    // returns a last-index, not a count.
    pub(crate) fn count(&self) -> usize {
        self.values.len() - 1
    }
}
