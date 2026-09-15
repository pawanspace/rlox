//! Hand-rolled manual memory management for heap-allocated values (mainly
//! strings).
//!
//! ## WHY this file exists (and why it is unusual for Rust)
//! This is a direct translation of clox's C memory model. In C you manage the
//! heap yourself: ask the allocator for N raw bytes, get back a `char*`, and
//! copy data in/out by hand. This file recreates that: it hands out raw
//! `*mut u8` pointers from the global allocator and copies bytes around with
//! `unsafe`. A `FatPointer` (see `common.rs`) then bundles such a raw pointer
//! with a length and a cached hash, playing the role of clox's `ObjString`.
//!
//! ## What Rust normally does instead
//! In idiomatic Rust you would never write this. `String`/`Vec<u8>`/`Box<str>`
//! already *are* safe, owned heap allocations: they track their own length and
//! capacity, free themselves automatically when dropped (RAII), and never let
//! you read out of bounds. Using them would delete this entire file and the
//! `unsafe` blocks with it. It is kept here deliberately as a learning exercise
//! — to feel where C-style manual memory and Rust's ownership model collide.
//!
//! ## The core primitives
//! - `Layout` — describes the size and alignment of an allocation. The
//!   allocator needs both: `size` is how many bytes, `align` is what address
//!   boundary the block must start on.
//! - `alloc(layout)` / `dealloc(ptr, layout)` — the low-level global allocator
//!   entry points. `dealloc` MUST be given the *same* `Layout` that `alloc`
//!   was, or behaviour is undefined.
//! - `*mut u8` — an untyped raw pointer to the first byte. Rust will not let
//!   you dereference it outside `unsafe`, because the compiler can no longer
//!   prove it is valid, aligned, initialised, or still alive.
//!
//! ## Safety status of this module (important)
//! Every function here is `unsafe` in spirit even when not marked `unsafe fn`.
//! There is currently NO tracking of which allocations are live and NOTHING
//! frees them (`drop` below is never called), so all allocations leak. And see
//! the BUG note on `allocate` — the sizing is wrong for strings.

use std::alloc::{alloc, dealloc, Layout};
use std::fmt::Debug;
use std::mem;

/// Allocate `len` raw bytes on the heap and return a pointer to them.
///
/// This is the byte-oriented allocator used to store string *contents*. The
/// caller passes the exact number of bytes it needs (e.g. `str.len()`), so the
/// allocation is always sized to the actual text — unlike a type-based
/// `Layout::new::<T>()`, which would give the size of a *type* (`size_of::<T>()`)
/// rather than a dynamic byte count.
///
/// Alignment is `1` because the buffer holds bytes (`u8`), which can live at any
/// address. That alignment MUST match the one used to free it — see
/// [`drop_bytes`], which also uses `from_size_align(len, 1)`.
///
/// `from_size_align` returns a `Result` (it can fail if the alignment isn't a
/// power of two or the rounded size overflows); with alignment `1` this only
/// fails on absurd lengths, so we `unwrap`.
pub fn allocate_bytes(len: usize) -> *mut u8 {
    let layout = Layout::from_size_align(len, 1).unwrap();
    unsafe {
        // `alloc` returns a null pointer on allocation failure rather than
        // panicking, so we must check it ourselves.
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("Unable to allocate pointer for layout {:?}", layout);
        }
        ptr
    }
}

/// Allocate a block sized for a specific *value* (`Layout::for_value` inspects
/// the value's runtime size) and return a raw pointer to it.
///
/// Unlike `allocate`, this consumes `value` only to measure it — it does not
/// write `value` into the returned memory.
pub fn allocate_for_value<T>(value: T) -> *mut u8 {
    let layout = Layout::for_value::<T>(&value);
    unsafe {
        let ptr = alloc(layout);
        if ptr.is_null() {
            panic!("Unable to allocate pointer for layout {:?}", layout);
        }
        ptr
    }
}

/// Write `value` into the memory at `ptr`, treating `ptr` as a `*mut T`.
///
/// `std::ptr::write` stores the bytes without reading or dropping whatever was
/// there before — correct here because the target is freshly allocated,
/// uninitialised memory (running a destructor on garbage would be UB).
pub fn add<T>(ptr: *mut u8, value: T) {
    unsafe {
        std::ptr::write(ptr as *mut T, value);
    }
}

/// Intended to report the size of the allocation `ptr` points at.
///
/// BUG: `mem::size_of_val(&ptr)` measures the size of the *pointer variable*
/// (`&ptr` is a reference to the `*mut u8`), which is always 8 bytes on a
/// 64-bit target — not the size of the pointee. This does not do what the name
/// suggests. (It appears unused.)
pub fn size_of<T>(ptr: *mut u8) -> usize {
    mem::size_of_val(&ptr)
}

/// Pointer *identity* comparison: true only if both pointers hold the same
/// address. It does NOT compare the bytes they point at. String interning (in
/// the compiler + hash table) relies on this: identical strings are stored
/// once, so "same address" can stand in for "same string".
pub fn eq(ptr: *mut u8, other_ptr: *mut u8) -> bool {
    std::ptr::eq(ptr, other_ptr)
}

/// Debug-print the byte at `ptr`.
///
/// NOTE: the type parameter `T` is unused — the body dereferences `ptr` as the
/// `u8` it is and prints that single byte, regardless of `T`.
pub fn print<T>(ptr: *mut u8)
where
    T: Debug,
{
    unsafe {
        println!("{:?}", *ptr);
    }
}

/// Free an allocation previously produced by `allocate::<T>()`.
///
/// It rebuilds `Layout::new::<T>()` because `dealloc` must be handed the exact
/// same layout the block was allocated with.
///
/// NOTE: nothing in the codebase currently calls this, so every allocation
/// leaks. A real VM frees strings/objects either eagerly or via a garbage
/// collector (clox adds a GC in a later chapter). It also inherits the sizing
/// BUG from `allocate` — freeing with the `String`-struct layout would not
/// match a correctly byte-sized allocation.
pub fn drop<T>(ptr: *mut u8) {
    let layout = Layout::new::<T>();
    unsafe {
        dealloc(ptr, layout);
    }
}

/// Free a buffer previously produced by [`allocate_bytes`].
///
/// `dealloc` requires the *exact same* `Layout` used to allocate — same size and
/// same alignment — so this rebuilds `from_size_align(len, 1)` to match
/// `allocate_bytes`. Because a byte allocation records no length of its own, the
/// caller must remember it; the VM keeps it in `FatPointer.size`.
///
/// NOTE: nothing calls this yet — string memory currently leaks. Freeing is only
/// safe once something tracks when a string is truly unreachable (Chapter 26,
/// garbage collection); freeing an interned string that is still referenced
/// elsewhere would be a use-after-free.
pub fn drop_bytes(ptr: *mut u8, len: usize) {
    let layout = Layout::from_size_align(len, 1).unwrap();
    unsafe {
        dealloc(ptr, layout);
    }
}

/// Reconstruct a Rust `String` from `len` raw bytes starting at `ptr`.
///
/// This is how the VM turns a stored `FatPointer` back into readable text
/// (for printing, comparison, hashing). It copies the bytes out one at a time
/// and validates them as UTF-8.
pub fn read_string(ptr: *mut u8, len: usize) -> String {
    unsafe {
        let mut bytes: Vec<u8> = Vec::new();
        for i in 0..len {
            // `ptr.offset(i)` advances the raw pointer by `i` bytes (pointer
            // arithmetic). Reading past the real allocation length is UB — the
            // caller is trusted to pass the correct `len`.
            let b = *(ptr.offset(i as isize));
            bytes.push(b);
        }
        // Strings in this VM must be valid UTF-8; a decode failure means the
        // stored bytes were corrupted (see the `allocate` overflow BUG).
        match String::from_utf8(bytes) {
            Ok(value) => value,
            Err(e) => panic!("not able to unwrap string from utf8 {:?}", e),
        }
    }
}

/// Read a `T` out of `ptr` by value.
///
/// `std::ptr::read` performs a bitwise copy out of the pointee without touching
/// the original memory. The caller is responsible for making sure the bytes at
/// `ptr` really are a valid, initialised `T`.
pub fn get<T>(ptr: *mut T) -> T {
    unsafe { std::ptr::read(ptr) }
}

/// Copy `length` bytes from `src` to `dest + offset`.
///
/// Used to build string storage: allocate a block, then `copy` the source
/// bytes in. The `offset` lets the caller place a second chunk right after a
/// first — this is how `concat` (in the VM) lays two strings end to end into
/// one allocation.
///
/// `copy_nonoverlapping` is the fast path that assumes the source and
/// destination byte ranges do not overlap (like C's `memcpy`, not `memmove`);
/// if they did overlap the result would be corrupt.
pub fn copy(src: *const u8, dest: *mut u8, length: usize, offset: usize) {
    unsafe { std::ptr::copy_nonoverlapping(src, dest.offset(offset as isize), length) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn can_allocated_long_string() {
        let mut src = String::from(
            "hello world is so old that I am just getting tired of it now and its not working",
        );
        let ptr = allocate_bytes(src.len());
        copy(src.as_mut_ptr(), ptr, src.len(), 0);
        let value = read_string(ptr, src.len());
        assert_eq!(value, src);
    }

    #[test]
    fn can_drop_allocated_bytes() {
        let ptr = allocate_bytes(10);
        drop_bytes(ptr, 10);
    }
}
