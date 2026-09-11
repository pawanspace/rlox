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

    /// Append a constant. Its index is then available via `last_index`.
    pub(crate) fn append(&mut self, value: common::Value) {
        self.values.push(value);
    }

    /// Look up a constant by index. Clones because `Value` is returned by value.
    pub(crate) fn get(&self, index: usize) -> common::Value {
        // TODO: `unwrap` panics on an out-of-range index; a real implementation
        // would return an error or `Option` instead of trusting the bytecode.
        (*self.values.get(index).unwrap()).clone()
    }

    /// Index of the most-recently-appended constant (`len() - 1`), used right
    /// after `append` to get the new constant's index. Returns `0` for an empty
    /// pool as a safe placeholder — callers only call this after an `append`, so
    /// the empty case never actually arises.
    pub(crate) fn last_index(&self) -> usize {
        match self.values.is_empty() {
            true => 0,
            false => self.values.len() - 1,
        }
    }
}
